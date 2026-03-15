package com.termux.rust

import org.gradle.api.Plugin
import org.gradle.api.Project
import org.gradle.api.tasks.Exec
import com.android.build.gradle.LibraryExtension

class RustAndroidPlugin implements Plugin<Project> {
    void apply(Project project) {
        project.extensions.create("rust", RustExtension)

        project.afterEvaluate {
            def rustExt = project.extensions.getByType(RustExtension)
            def rustSrcDir = project.file(rustExt.rustDir)
            def jniLibsDir = project.file("src/main/jniLibs")

            // ABI 到 Rust Target 的映射（用于定位编译产物）
            def abiToTarget = [
                "arm64-v8a": "aarch64-linux-android",
                "armeabi-v7a": "armv7-linux-androideabi",
                "x86_64": "x86_64-linux-android",
                "x86": "i686-linux-android"
            ]

            def buildAllRust = project.task("buildAllRust")

            abiToTarget.each { abi, target ->
                def buildTask = project.tasks.create("buildRust-${abi}", Exec) {
                    group = "rust"
                    workingDir = rustSrcDir
                    
                    def buildType = project.gradle.startParameter.taskNames.any { it.contains("Release") } ? "release" : "debug"
                    def cargoFlags = buildType == "release" ? ["--release"] : []
                    
                    // 同步您之前的参数：使用 cargo-ndk, 指定 API 24
                    commandLine "cargo", "ndk", "-t", abi, "-p", "24", "build", *cargoFlags
                    
                    inputs.dir(project.file("${rustExt.rustDir}/src"))
                    inputs.file(project.file("${rustExt.rustDir}/Cargo.toml"))
                    outputs.dir(project.file("${rustExt.rustDir}/target/${target}/${buildType}"))

                    doLast {
                        def libName = "lib${rustExt.moduleName}.so"
                        def sourceLib = project.file("${rustExt.rustDir}/target/${target}/${buildType}/${libName}")
                        def destDir = project.file("${jniLibsDir}/${abi}")
                        
                        if (sourceLib.exists()) {
                            project.mkdir(destDir)
                            project.copy {
                                from sourceLib
                                into destDir
                            }
                        } else {
                            println "Warning: Could not find Rust library at ${sourceLib.absolutePath}"
                        }
                    }
                }
                buildAllRust.dependsOn(buildTask)
            }

            // 挂载到 Android 编译生命周期
            project.tasks.configureEach { task ->
                if (task.name.startsWith("merge") && task.name.endsWith("JniLibFolders")) {
                    task.dependsOn(buildAllRust)
                }
            }
        }
    }
}

class RustExtension {
    String rustDir = "src/main/rust"
    String moduleName = "termux_rust"
}
