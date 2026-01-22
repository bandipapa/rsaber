// TODO: Check if we have any unneeded artifact in release apk.

import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive

val ID = "com.bandipapa.rsaber"
val ARCH = "arm64-v8a"
val PLATFORM = "aarch64-linux-android"
val ANDROID_SDK_LEVEL = System.getenv("ANDROID_SDK_LEVEL").toInt()
val CARGO_METADATA = readCargoMetadata()
val VERSION = getVersion()

plugins {
    alias(libs.plugins.android.application)
}

buildscript {
    dependencies {
        classpath(libs.kotlinx.serialization.json)
    }
}

android {
    namespace = ID
    compileSdk = ANDROID_SDK_LEVEL

    defaultConfig {
        applicationId = ID
        minSdk = ANDROID_SDK_LEVEL
        targetSdk = ANDROID_SDK_LEVEL
        versionName = VERSION
        versionCode = getVersionCode()

        ndk {
            abiFilters.clear()
            abiFilters += ARCH // Remove unneeded arch from OpenXR loader.
        }

        signingConfigs {
            create("release") {
                storeFile = if (System.getenv("ANDROID_KEYSTORE") != null) file(System.getenv("ANDROID_KEYSTORE")) else null
                storePassword = System.getenv("ANDROID_KEYSTORE_PW")
                keyAlias = System.getenv("ANDROID_KEYALIAS")
                keyPassword = System.getenv("ANDROID_KEYALIAS_PW")
            }
        }

        buildTypes {
            debug {
                isDebuggable = true
            }
      
            release {
                signingConfig = signingConfigs.getByName("release")
            }
        }
    }
}

dependencies {
    runtimeOnly(libs.openxr.loader)
}

tasks {
    register<Exec>("rustDebug") {
        workingDir = file("../..")
        commandLine = execCargo(false)
    }

    register<Exec>("rustRelease") {
        workingDir = file("../..")
        commandLine = execCargo(true)
    }

    whenTaskAdded { 
        if (name == "preDebugBuild")
            dependsOn("rustDebug")
    }

    whenTaskAdded { 
        if (name == "preReleaseBuild")
            dependsOn("rustRelease")
    }

    register<Exec>("runDebug") {
        dependsOn("installDebug")
        commandLine = run()
    }

    register<Exec>("runRelease") {
        dependsOn("installRelease")
        commandLine = run()
    }
}

fun readCargoMetadata(): JsonObject {
    val output = providers.exec {
        workingDir = file("../..")
        commandLine("cargo", "metadata", "--format-version", "1", "--filter-platform", PLATFORM)
    }.standardOutput.asText
    return Json.decodeFromString<JsonObject>(output.get())
}

fun getVersion(): String {
    return CARGO_METADATA
        .getValue("packages")
        .jsonArray
        .first {
            element -> element.jsonObject.getValue("name").jsonPrimitive.content == "rsaber_android"
        }.jsonObject.getValue("version").jsonPrimitive.content
}

fun getVersionCode(): Int {
    val (major, minor, patch) = VERSION.split(".").map { it.toInt() }
    return major * 256 * 256 + minor * 256 + patch
}

fun execCargo(release: Boolean): List<String> {
    return buildList {
        add("cargo")
        add("ndk")
        add("-q")
        add("-t")
        add(ARCH)
        add("build")
        add("-o")
        add("build/app/src/" + (if (release) "release" else "debug") + "/jniLibs")
        add("-P")
        add(ANDROID_SDK_LEVEL.toString())
        if (release) add("--release")
    }
}

fun run(): List<String> {
    val adb = androidComponents.sdkComponents.adb.get().asFile.getAbsolutePath()
    return listOf(adb, "shell", "am", "start", "-a", "android.intent.action.MAIN", "-n", ID + "/android.app.NativeActivity")
}
