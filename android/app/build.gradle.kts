import java.util.Properties

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

// Test builds (android/build-apk.sh --test): a separate app, "Sequensss Test", installed
// next to the stable one, with its own version.
val testBuild = (findProperty("sequensss.test") as String?) == "true"

android {
    namespace = "com.sequensss.app"
    compileSdk = 35

    defaultConfig {
        applicationId = "com.sequensss.app"
        minSdk = 29          // AMidi (native MIDI) is available from API 29
        targetSdk = 35
        versionCode = 5
        versionName = "0.1.4"
        manifestPlaceholders["appName"] = "Sequensss"
        if (testBuild) {
            applicationIdSuffix = ".test"
            manifestPlaceholders["appName"] = "Sequensss Test"
            versionCode = (findProperty("sequensss.versionCode") as String).toInt()
            versionName = findProperty("sequensss.versionName") as String
        }
        ndk {
            abiFilters += listOf("arm64-v8a", "x86_64")
        }
    }

    // Release signing key: android/keystore.properties (not in git); unsigned release otherwise.
    val keystoreProps = rootProject.file("keystore.properties")
    if (keystoreProps.exists()) {
        val props = Properties().apply { keystoreProps.inputStream().use { load(it) } }
        signingConfigs {
            create("release") {
                storeFile = file(props.getProperty("storeFile"))
                storePassword = props.getProperty("storePassword")
                keyAlias = props.getProperty("keyAlias")
                keyPassword = props.getProperty("keyPassword")
            }
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            if (keystoreProps.exists()) signingConfig = signingConfigs.getByName("release")
        }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions {
        jvmTarget = "17"
    }
    // The Rust library (libseq_android.so) is copied into src/main/jniLibs by cargo-ndk.
}

dependencies {
    // Version required by android-activity 0.6 (do NOT enable GameActivity's C++ prefab).
    implementation("androidx.games:games-activity:4.4.0")
    implementation("androidx.appcompat:appcompat:1.7.0")
    implementation("androidx.core:core-ktx:1.13.1")
}
