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

            def targets = [
                "arm64-v8a": "aarch64-linux-android",
                "armeabi-v7a": "armv7-linux-androideabi",
                "x86_64": "x86_64-linux-android",
                "x86": "i686-linux-android"
            ]

            def buildAllRust = project.task("buildAllRust")

            targets.each { abi, target ->
                def buildTask = project.tasks.create("buildRust-${abi}", Exec) {
                    group = "rust"
                    workingDir = rustSrcDir
                    
                    def buildType = project.gradle.startParameter.taskNames.any { it.contains("Release") } ? "release" : "debug"
                    def cargoFlags = buildType == "release" ? ["--release"] : []
                    
                    commandLine "cargo", "build", "--target", target, *cargoFlags
                    
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
                        }
                    }
                }
                buildAllRust.dependsOn(buildTask)
            }

            // Gradle 8.x/9.x 任务挂载
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
