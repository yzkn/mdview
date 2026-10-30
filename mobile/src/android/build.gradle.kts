// mdview のモバイル版の、端末の書類（SAF）・出力・共有の部品（v3 R-02）。
// dx（Dioxus の CLI）が APK を組むときに取り込む（manganis::ffi。src/native.rs）
plugins {
    id("com.android.library") version "8.4.2"
    kotlin("android") version "1.9.24"
}

android {
    // **この名前がクラスの道筋になる**（src/native.rs の CLASS と合わせる）
    namespace = "jp.mdview.mobile.documents"
    compileSdk = 35

    defaultConfig {
        minSdk = 26
        consumerProguardFiles("consumer-rules.pro")
    }

    buildTypes {
        getByName("release") { isMinifyEnabled = false }
        getByName("debug") { isMinifyEnabled = false }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    kotlinOptions {
        jvmTarget = "17"
    }
}

dependencies {
    // アプリの側（dx の雛形）が appcompat を入れるので、同じ部品を使う
    implementation("androidx.activity:activity-ktx:1.9.3")
    implementation("androidx.core:core-ktx:1.13.1")
}
