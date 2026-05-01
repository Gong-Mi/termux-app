plugins {
    id("com.android.library")
    id("termux.rust.android")
}

android {
    namespace = "com.termux.emulator"

    val ndkVersion: String by project
    this.ndkVersion = ndkVersion

    defaultConfig {
        val minSdkVersion: String by project
        val compileSdkVersion: String by project
        minSdk = minSdkVersion.toInt()
        compileSdk = compileSdkVersion.toInt()
        ndk {
            abiFilters += listOf("x86", "x86_64", "armeabi-v7a", "arm64-v8a")
        }
    }

    buildTypes {
        getByName("release") {
            isMinifyEnabled = false
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_21
        targetCompatibility = JavaVersion.VERSION_21
    }
}

rustAndroid {
    rustSrcDir = "src/main/rust"
    jniLibsDestDir = "src/main/jniLibs"
    libName = "termux_rust"
    minSdkVersion = providers.gradleProperty("minSdkVersion").get().toInt()
    abiFilters = listOf("x86", "x86_64", "armeabi-v7a", "arm64-v8a")
}

dependencies {
    implementation("androidx.annotation:annotation:1.9.1")
    testImplementation("junit:junit:4.13.2")
}
