// The Java half of the app: the activity, the playback service and the resources. The player itself is
// the Rust library in ../src, built first with cargo-ndk into src/main/jniLibs (see android/README.md).

plugins {
    id("com.android.application")
}

android {
    namespace = "io.github.dividedswhimsicalvhs.wispplayer"
    compileSdk = 35

    defaultConfig {
        applicationId = "io.github.dividedswhimsicalvhs.wispplayer"
        minSdk = 26 // the lowest Slint's Android backend supports
        targetSdk = 35
        versionCode = 200 // 0.2.0
        versionName = "0.2.0"

        // The ABIs to package, comma separated: -PwispAbis=arm64-v8a,x86_64. Phones are arm64-v8a; the
        // emulator on a computer is x86_64.
        ndk {
            abiFilters += (project.findProperty("wispAbis") as String? ?: "arm64-v8a").split(",")
        }
    }

    // The release key comes from the environment, so it never has to be in the repository.
    val keystore = System.getenv("WISP_KEYSTORE")
    if (keystore != null) {
        signingConfigs {
            create("release") {
                storeFile = file(keystore)
                storePassword = System.getenv("WISP_KEYSTORE_PASSWORD")
                keyAlias = System.getenv("WISP_KEY_ALIAS")
                keyPassword = System.getenv("WISP_KEY_PASSWORD")
            }
        }
    }

    buildTypes {
        release {
            if (keystore != null) {
                signingConfig = signingConfigs.getByName("release")
            }
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}
