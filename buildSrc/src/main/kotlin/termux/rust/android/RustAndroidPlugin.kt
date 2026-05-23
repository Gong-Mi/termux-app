package termux.rust.android

import java.io.File
import org.gradle.api.Plugin
import org.gradle.api.Project
import org.gradle.api.provider.ListProperty
import org.gradle.api.provider.Property
import org.gradle.api.tasks.Copy
import org.gradle.api.tasks.Exec
import org.gradle.kotlin.dsl.*

class RustAndroidPlugin : Plugin<Project> {
    override fun apply(project: Project) {
        val extension = project.extensions.create<RustAndroidExtension>("rustAndroid")

        fun rustInputs(srcDir: String) = project.fileTree(srcDir) {
            exclude("target/**")
        }

        // Read NDK path from local.properties (if available) to set ANDROID_NDK for cargo/skia builds
        var ndkPath = runCatching {
            val localProps = java.util.Properties()
            val localPropsFile = project.rootProject.file("local.properties")
            if (localPropsFile.exists()) {
                localPropsFile.inputStream().use { localProps.load(it) }
                localProps.getProperty("ndk.dir")
            } else null
        }.getOrNull()
        if (ndkPath.isNullOrBlank()) ndkPath = System.getenv("ANDROID_NDK_HOME")
        if (ndkPath.isNullOrBlank()) ndkPath = System.getenv("ANDROID_NDK_ROOT")

        fun configureRustBuilds(pluginId: String) {
            project.pluginManager.withPlugin(pluginId) {
                project.afterEvaluate {
                    val abiList = extension.abiFilters.get()
                        .ifEmpty { listOf("arm64-v8a", "armeabi-v7a", "x86", "x86_64") }

                    val rustSrc = extension.rustSrcDir.get()
                    val libName = extension.libName.get()
                    val jniDest = extension.jniLibsDestDir.get()
                    val minSdk = extension.minSdkVersion.get()

                    val buildTasks = abiList.map { abi ->
                        val rustArch = abiToRustTarget(abi)
                        val safeAbi = abi.replace("-", "_")
                        val taskName = "cargoNdkBuild_${safeAbi}"

                        project.tasks.register<Exec>(taskName) {
                            group = "rust"
                            description = "Build Rust $libName for $abi"

                            val targetDir = project.file("$rustSrc/target/$safeAbi")
                            inputs.files(rustInputs(rustSrc))
                            outputs.file(project.file("${targetDir.path}/$rustArch/release/lib$libName.so"))

                            workingDir = project.file(rustSrc)
                            
                            environment("CARGO_TARGET_DIR", targetDir.path)

                            if (!ndkPath.isNullOrBlank()) {
                                environment("ANDROID_NDK", ndkPath)
                            }

                            commandLine(
                                "cargo", "ndk",
                                "-t", abi,
                                "-p", minSdk.toString(),
                                "build", "--release"
                            )
                        }

                        // Also build termux-exec-rs if it exists
                        val execSrc = "src/main/rust-exec"
                        val execTaskName = "cargoNdkBuildExec_${safeAbi}"
                        if (project.file(execSrc).exists()) {
                            project.tasks.register<Exec>(execTaskName) {
                                group = "rust"
                                description = "Build Rust termux-exec for $abi"

                                val targetDir = project.file("$execSrc/target/$safeAbi")
                                inputs.files(rustInputs(execSrc))
                                outputs.file(project.file("${targetDir.path}/$rustArch/release/libtermux_exec.so"))

                                workingDir = project.file(execSrc)
                                environment("CARGO_TARGET_DIR", targetDir.path)

                                if (!ndkPath.isNullOrBlank()) {
                                    environment("ANDROID_NDK", ndkPath)
                                }

                                commandLine(
                                    "cargo", "ndk",
                                    "-t", abi,
                                    "-p", minSdk.toString(),
                                    "build", "--release"
                                )
                            }
                        }
                    }

                    val copyTasks = abiList.map { abi ->
                        val rustArch = abiToRustTarget(abi)
                        val safeAbi = abi.replace("-", "_")
                        val buildTaskName = "cargoNdkBuild_${safeAbi}"
                        val execBuildTaskName = "cargoNdkBuildExec_${safeAbi}"
                        val copyTaskName = "copyRust_${safeAbi}"

                        project.tasks.register<Copy>(copyTaskName) {
                            group = "rust"
                            val mainTargetDir = project.file("$rustSrc/target/$safeAbi")
                            val execTargetDir = project.file("src/main/rust-exec/target/$safeAbi")
                            
                            dependsOn(buildTaskName)
                            if (project.tasks.findByName(execBuildTaskName) != null) {
                                dependsOn(execBuildTaskName)
                            }

                            from(project.file("${mainTargetDir.path}/$rustArch/release/lib$libName.so"))
                            into(project.file("$jniDest/$abi"))

                            // Copy termux-exec if built
                            if (project.file("src/main/rust-exec").exists()) {
                                from(project.file("${execTargetDir.path}/$rustArch/release/libtermux_exec.so")) {
                                    rename { "libtermux-exec.so" }
                                }
                            }

                            // Also copy libc++_shared.so from NDK if available
                            if (!ndkPath.isNullOrBlank()) {
                                val triple = when (abi) {
                                    "arm64-v8a" -> "aarch64-linux-android"
                                    "armeabi-v7a" -> "arm-linux-androideabi"
                                    "x86" -> "i686-linux-android"
                                    "x86_64" -> "x86_64-linux-android"
                                    else -> rustArch
                                }
                                val cxxShared = File(ndkPath, "toolchains/llvm/prebuilt/linux-x86_64/sysroot/usr/lib/$triple/libc++_shared.so")
                                if (cxxShared.exists()) {
                                    from(cxxShared)
                                }
                            }
                        }
                    }

                    // Copy standalone Rust binaries (e.g., termux_exec_device_probe) into assets
                    val binaryCopyTasks = abiList.map { abi ->
                        val rustArch = abiToRustTarget(abi)
                        val safeAbi = abi.replace("-", "_")
                        val execBuildTaskName = "cargoNdkBuildExec_${safeAbi}"
                        val binaryCopyTaskName = "copyRustBinary_${safeAbi}"
                        val execTargetDir = project.file("src/main/rust-exec/target/$safeAbi")
                        val assetsDest = project.file("src/main/assets/termux-probes/$abi")

                        project.tasks.register<Copy>(binaryCopyTaskName) {
                            group = "rust"
                            description = "Copy Rust standalone binaries for $abi into assets"

                            onlyIf {
                                project.tasks.findByName(execBuildTaskName) != null
                            }
                            dependsOn(execBuildTaskName)

                            from(project.file("${execTargetDir.path}/$rustArch/release/termux_exec_device_probe"))
                            into(assetsDest)
                        }
                    }

                    project.tasks.register("buildAllRust") {
                        group = "rust"
                        description = "Build Rust $libName for all ABIs"
                        dependsOn(copyTasks)
                        dependsOn(binaryCopyTasks)
                    }

                    // Hook into merge*JniLibFolders and merge*Assets
                    project.tasks.configureEach {
                        if (name.startsWith("merge") && (name.endsWith("JniLibFolders") || name.endsWith("Assets"))) {
                            dependsOn("buildAllRust")
                        }
                    }
                }
            }
        }

        configureRustBuilds("com.android.library")
        configureRustBuilds("com.android.application")
    }

    private fun abiToRustTarget(abi: String): String = when (abi) {
        "arm64-v8a" -> "aarch64-linux-android"
        "armeabi-v7a" -> "armv7-linux-androideabi"
        "x86" -> "i686-linux-android"
        "x86_64" -> "x86_64-linux-android"
        else -> throw IllegalArgumentException("Unknown ABI: $abi")
    }
}

abstract class RustAndroidExtension {
    abstract val rustSrcDir: Property<String>
    abstract val jniLibsDestDir: Property<String>
    abstract val libName: Property<String>
    abstract val minSdkVersion: Property<Int>
    abstract val abiFilters: ListProperty<String>

    init {
        rustSrcDir.convention("src/main/rust")
        jniLibsDestDir.convention("src/main/jniLibs")
        libName.convention("termux_rust")
        minSdkVersion.convention(24)
        abiFilters.convention(emptyList())
    }
}
