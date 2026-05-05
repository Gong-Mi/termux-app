import org.gradle.api.internal.file.FileOperations
import java.io.BufferedOutputStream
import java.io.FileInputStream
import java.io.FileOutputStream
import java.net.URI
import java.security.DigestInputStream
import java.security.MessageDigest

plugins {
    id("com.android.application")
}

val packageVariant = System.getenv("TERMUX_PACKAGE_VARIANT") ?: "apt-android-7"

// 获取 git commit 短哈希，用于版本追溯
fun getGitHash(): String {
    return try {
        Runtime.getRuntime().exec(arrayOf("git", "rev-parse", "--short", "HEAD"))
            .inputStream.bufferedReader().readText().trim()
    } catch (e: Exception) {
        "unknown"
    }
}

val gitHash = getGitHash()
val baseVersionName = "0.118.0"
val appVersionName = System.getenv("TERMUX_APP_VERSION_NAME") ?: "$baseVersionName+$gitHash"
val apkVersionTag = System.getenv("TERMUX_APK_VERSION_TAG") ?: "v$appVersionName-$packageVariant"

android {
    namespace = "com.termux"

    val ndkVersion: String by project
    this.ndkVersion = ndkVersion

    defaultConfig {
        versionCode = 118
        versionName = appVersionName

        val minSdkVersion: String by project
        val targetSdkVersion: String by project
        val compileSdkVersion: String by project
        minSdk = minSdkVersion.toInt()
        targetSdk = targetSdkVersion.toInt()
        compileSdk = compileSdkVersion.toInt()
        ndk {
            if (project.hasProperty("abiFilter")) {
                abiFilters += listOf(project.property("abiFilter") as String)
            } else {
                abiFilters += listOf("armeabi-v7a", "arm64-v8a", "x86_64")
            }
        }

        buildConfigField("String", "TERMUX_PACKAGE_VARIANT", "\"$packageVariant\"")
    }

    signingConfigs {
        getByName("debug") {
            storeFile = file("testkey_untrusted.jks")
            keyAlias = "alias"
            storePassword = "xrj45yWGLbsO7W0v"
            keyPassword = "xrj45yWGLbsO7W0v"
        }
    }

    buildTypes {
         getByName("release") {
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
        }

        getByName("debug") {
            signingConfig = signingConfigs.getByName("debug")
        }
    }

    buildFeatures {
        buildConfig = true
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_21
        targetCompatibility = JavaVersion.VERSION_21
    }


    testOptions {
        unitTests {
            isIncludeAndroidResources = true
        }
    }

    packaging {
        jniLibs {
            useLegacyPackaging = true
        }
    }

    lint {
        warningsAsErrors = true
    }

    applicationVariants.all {
        outputs.all {
            val output = this as com.android.build.gradle.internal.api.BaseVariantOutputImpl
            val abi = output.getFilter(com.android.build.OutputFile.ABI) ?: "universal"
            val buildType = variant.buildType.name
            val tag = System.getenv("TERMUX_APK_VERSION_TAG") ?: "v${android.defaultConfig.versionName}-$packageVariant-$buildType"
            output.outputFileName = "termux-app_${tag}_${abi}.apk"
        }
    }
}

// Bootstrap zip is embedded in libtermux_rust.so via build.rs + include_bytes!().
// The legacy C ndkBuild and libtermux-bootstrap.so have been removed.

dependencies {
    implementation("androidx.annotation:annotation:1.10.0")
    implementation("androidx.core:core:1.18.0")
    implementation("androidx.drawerlayout:drawerlayout:1.2.0")
    implementation("androidx.viewpager:viewpager:1.1.0")
    implementation("com.google.android.material:material:1.13.0")
    implementation(project(":terminal-view"))

    testImplementation("junit:junit:4.13.2")
    testImplementation("org.robolectric:robolectric:4.16.1")
}

tasks.register("versionName") {
    doLast {
        print(android.defaultConfig.versionName)
    }
}

abstract class CleanBootstrapsTask : DefaultTask() {
    @get:InputDirectory abstract val projectDir: DirectoryProperty
    @get:Inject abstract val fileOperations: FileOperations

    @TaskAction
    fun action() {
        val tree = fileOperations.fileTree(File(projectDir.asFile.get(), "src/main/cpp"))
        tree.include("**/bootstrap-*.zip")
        tree.include("**/libproot-*.so")
        tree.forEach { it.delete() }
    }
}

tasks.register<CleanBootstrapsTask>("cleanBootstraps") {
    projectDir = layout.projectDirectory
}

tasks.named("clean") {
    dependsOn("cleanBootstraps")
}

tasks.register("downloadBootstraps") {
    val projectDirPath = project.layout.projectDirectory.asFile.absolutePath
    val variant = packageVariant
    doLast {
        val projectDir = File(projectDirPath)

        fun download(arch: String, defaultChecksum: String, version: String) {
            val digest = MessageDigest.getInstance("SHA-256")
            val localUrl = "src/main/cpp/bootstrap-$arch.zip"
            val file = File(projectDir, localUrl)

            val sha256File = File(projectDir, "$localUrl.sha256sum")
            val expectedChecksum = if (sha256File.exists()) {
                val content = sha256File.readText().trim()
                val hash = content.split(Regex("\\s+")).first()
                println("Using local sha256sum for bootstrap-$arch.zip: $hash")
                hash
            } else {
                defaultChecksum
            }

            if (file.exists()) {
                val buffer = ByteArray(8192)
                val input = FileInputStream(file)
                while (true) {
                    val readBytes = input.read(buffer)
                    if (readBytes < 0) break
                    digest.update(buffer, 0, readBytes)
                }
                var checksum = BigInteger(1, digest.digest()).toString(16)
                while (checksum.length < 64) { checksum = "0$checksum" }
                if (checksum == expectedChecksum) {
                    return
                } else {
                    println("Deleting old local file with wrong hash: $localUrl: expected: $expectedChecksum, actual: $checksum")
                    file.delete()
                }
            }

            val remoteUrl = "https://github.com/Gong-Mi/termux-packages/releases/download/bootstrap-$version/bootstrap-$arch.zip"
            println("Downloading $remoteUrl ...")

            file.parentFile.mkdirs()
            val out = BufferedOutputStream(FileOutputStream(file))
            val connection = URI(remoteUrl).toURL().openConnection()
            val digestStream = DigestInputStream(connection.inputStream, digest)
            digestStream.transferTo(out)
            out.close()

            var checksum = BigInteger(1, digest.digest()).toString(16)
            while (checksum.length < 64) { checksum = "0$checksum" }
            if (checksum != expectedChecksum) {
                file.delete()
                throw GradleException("Wrong checksum for $remoteUrl:\n Expected: $expectedChecksum\n Actual:   $checksum")
            }
        }

        fun downloadProot(localDir: String, arch: String, expectedChecksum: String) {
            val digest = MessageDigest.getInstance("SHA-256")
            val file = File(projectDir, "src/main/jniLibs/$localDir/libproot-loader.so")

            if (file.exists()) {
                val buffer = ByteArray(8192)
                val input = FileInputStream(file)
                while (true) {
                    val readBytes = input.read(buffer)
                    if (readBytes < 0) break
                    digest.update(buffer, 0, readBytes)
                }
                var checksum = BigInteger(1, digest.digest()).toString(16)
                while (checksum.length < 64) { checksum = "0$checksum" }
                if (checksum == expectedChecksum) return
                println("Deleting old proot loader with wrong hash: src/main/jniLibs/$localDir/libproot-loader.so")
                file.delete()
            }

            val prootTag = "proot-2026.01.22-r1"
            val prootVersion = "5.1.107-70"
            val remoteUrl = "https://github.com/termux-play-store/termux-packages/releases/download/$prootTag/libproot-loader-$arch-$prootVersion.so"
            println("Downloading $remoteUrl ...")

            file.parentFile.mkdirs()
            val out = BufferedOutputStream(FileOutputStream(file))
            val connection = URI(remoteUrl).toURL().openConnection()
            val digestStream = DigestInputStream(connection.inputStream, digest)
            digestStream.transferTo(out)
            out.close()

            var checksum = BigInteger(1, digest.digest()).toString(16)
            while (checksum.length < 64) { checksum = "0$checksum" }
            if (checksum != expectedChecksum) {
                file.delete()
                throw GradleException("Wrong checksum for $remoteUrl:\n Expected: $expectedChecksum\n Actual:   $checksum")
            }
        }

        downloadProot("armeabi-v7a", "arm", "09729047155df0c1a6b55c265ff4e272107775961d7efaff06bdd7cf37904050")
        downloadProot("arm64-v8a", "aarch64", "f7e3211e4c210c2a39a1f22b7f38666d99aee172fd009c0d19b84108cf20bb42")
        downloadProot("x86_64", "x86_64", "86e22d456255417e1d4ee874986571578ff26675ae2e372458e0d87f26454c63")

        if (variant == "apt-android-7") {
            val version = "2026.03.01-r1+apt.android-7"
            download("aarch64", "dd2040ad9ba1445eaf0818f3305bf190e8bdd04bcc0019faf0279181c48e71e3", version)
            download("arm",     "b8bdc78f2d22c63bf32d51daf02d2128685427abe72341884704060a3fe654a7", version)
            download("i686",    "cc44c1d405d4adff679cb23d3c4555be8a254b517000e894bfed87e182335fa8", version)
            download("x86_64",  "6dad9cd8317e9b2a474f9234f7013857a5813cfd2d28d1d57b169d5cdc09570d", version)
        } else if (variant == "apt-android-5") {
            val version = "2022.04.28-r6+" + variant
            download("aarch64", "913609d439415c828c5640be1b0561467e539cb1c7080662decaaca2fb4820e7", version)
            download("arm",     "26bfb45304c946170db69108e5eb6e3641aad751406ce106c80df80cad2eccf8", version)
            download("i686",    "46dcfeb5eef67ba765498db9fe4c50dc4690805139aa0dd141a9d8ee0693cd27", version)
            download("x86_64",  "615b590679ee6cd885b7fd2ff9473c845e920f9b422f790bb158c63fe42b8481", version)
        } else {
            throw GradleException("Unsupported TERMUX_PACKAGE_VARIANT \"$variant\"")
        }
    }
}

tasks.named("preBuild") {
    dependsOn("downloadBootstraps")
}
