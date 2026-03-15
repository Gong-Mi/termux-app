package com.termux.rust

import org.gradle.api.Plugin
import org.gradle.api.Project
import org.gradle.api.tasks.Exec
import org.gradle.api.tasks.Copy

class RustAndroidPlugin implements Plugin<Project> {
    void apply(Project project) {
        project.extensions.create("rustAndroid", RustAndroidExtension)

        project.afterEvaluate {
            def extension = project.extensions.getByType(RustAndroidExtension)
            def cargoTargetDir = project.file("${extension.rustSrcDir}/target")
            def jniLibsDir = project.file(extension.jniLibsDestDir)

            def ndkAbis = project.android.defaultConfig.ndk.abiFilters
            if (!ndkAbis) {
                ndkAbis = ['arm64-v8a', 'armeabi-v7a', 'x86', 'x86_64'] as Set
            }

            // Build Rust libraries for each ABI
            def cargoTasks = []
            ndkAbis.each { abi ->
                def rustArch = getRustArch(abi)
                def cargoTaskName = "cargoNdkBuild${abi.capitalize()}"
                def cargoTask = project.tasks.register(cargoTaskName, Exec) {
                    workingDir extension.rustSrcDir
                    
                    // 跟踪 Rust 源代码作为输入，确保增量构建生效
                    inputs.dir("${extension.rustSrcDir}/src")
                    inputs.file("${extension.rustSrcDir}/Cargo.toml")
                    if (project.file("${extension.rustSrcDir}/Cargo.lock").exists()) {
                        inputs.file("${extension.rustSrcDir}/Cargo.lock")
                    }
                    
                    // 指定输出文件
                    outputs.file("${cargoTargetDir}/${rustArch}/release/lib${extension.libName}.so")

                    commandLine 'cargo', 'ndk', '-t', abi, '-p', extension.minSdkVersion.toString(), 'build', '--release'

                    doFirst {
                        println "Compiling Rust for ABI: ${abi}..."
                    }
                }
                cargoTasks.add(cargoTask)
            }

            // Copy built .so files to jniLibs
            def copyTasks = []
            ndkAbis.each { abi ->
                def rustArch = getRustArch(abi)
                def copyTaskName = "copyRust${abi.capitalize()}"
                def copyTask = project.tasks.register(copyTaskName, Copy) {
                    dependsOn "cargoNdkBuild${abi.capitalize()}"
                    from "${cargoTargetDir}/${rustArch}/release/lib${extension.libName}.so"
                    into "${jniLibsDir}/${abi}"
                }
                copyTasks.add(copyTask)
            }

            def buildAllRust = project.tasks.register("buildAllRust") {
                dependsOn copyTasks
            }

            // Hook into Android build process - ensure Rust libs are built before mergeJniLibFolders
            project.tasks.configureEach { task ->
                if (task.name == "preBuild" || (task.name.startsWith("merge") && task.name.endsWith("JniLibFolders"))) {
                    task.dependsOn buildAllRust
                }
            }
        }
    }

    private String getRustArch(String abi) {
        switch (abi) {
            case 'arm64-v8a': return 'aarch64-linux-android'
            case 'armeabi-v7a': return 'armv7-linux-androideabi'
            case 'x86': return 'i686-linux-android'
            case 'x86_64': return 'x86_64-linux-android'
            default: throw new IllegalArgumentException("Unknown ABI: " + abi)
        }
    }
}

class RustAndroidExtension {
    String rustSrcDir = "src/main/rust"
    String jniLibsDestDir = "src/main/jniLibs"
    String libName = "termux_rust"
    int minSdkVersion = 24
}
