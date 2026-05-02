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

                            inputs.file(project.file("$rustSrc/Cargo.toml"))
                            inputs.dir(project.file("$rustSrc/src"))

                            val cargoLock = project.file("$rustSrc/Cargo.lock")
                            if (cargoLock.exists()) {
                                inputs.file(cargoLock)
                            }

                            outputs.file(
                                project.file("$rustSrc/target/$rustArch/release/lib$libName.so")
                            )

                            workingDir = project.file(rustSrc)

                            // Export ANDROID_NDK for skia-bindings and cargo-ndk
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

                    val copyTasks = abiList.map { abi ->
                        val rustArch = abiToRustTarget(abi)
                        val safeAbi = abi.replace("-", "_")
                        val buildTaskName = "cargoNdkBuild_${safeAbi}"
                        val copyTaskName = "copyRust_${safeAbi}"

                        project.tasks.register<Copy>(copyTaskName) {
                            group = "rust"
                            dependsOn(buildTaskName)

                            from(project.file("$rustSrc/target/$rustArch/release/lib$libName.so"))
                            into(project.file("$jniDest/$abi"))
                        }
                    }

                    project.tasks.register("buildAllRust") {
                        group = "rust"
                        description = "Build Rust $libName for all ABIs"
                        dependsOn(copyTasks)
                    }

                    // Hook into merge*JniLibFolders
                    project.tasks.configureEach {
                        if (name.startsWith("merge") && name.endsWith("JniLibFolders")) {
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
