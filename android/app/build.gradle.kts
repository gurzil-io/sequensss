import java.util.Properties

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

// Version: passed by android/build-apk.sh, otherwise read from the workspace Cargo.toml
// (the single source). versionCode = major * 10000 + minor * 100 + patch.
val cargoVersion: String by lazy {
    Regex("""(?m)^version = "([^"]+)"""").find(rootProject.file("../Cargo.toml").readText())!!.groupValues[1]
}
val appVersion = (findProperty("sequensss.versionName") as String?) ?: cargoVersion
val appVersionCode = (findProperty("sequensss.versionCode") as String?)?.toInt()
    ?: appVersion.substringBefore('-').split('.').map { it.toInt() }.let { (a, b, c) -> a * 10000 + b * 100 + c }

// Beta builds (android/build-apk.sh --beta): a separate app, "Sequensss Beta", installed
// next to the stable one.
val betaBuild = (findProperty("sequensss.beta") as String?) == "true"

android {
    namespace = "com.sequensss.app"
    compileSdk = 35

    defaultConfig {
        applicationId = "com.sequensss.app"
        minSdk = 29          // AMidi (native MIDI) is available from API 29
        targetSdk = 35
        versionCode = appVersionCode
        versionName = appVersion
        manifestPlaceholders["appName"] = "Sequensss"
        if (betaBuild) {
            applicationIdSuffix = ".beta"
            manifestPlaceholders["appName"] = "Sequensss Beta"
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
