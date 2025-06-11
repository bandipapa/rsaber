# rsaber: Beat Saber clone written in Rust

## State

We are really at the beginning. At least, the colored notes are already moving :).

## Supported Devices

| Subdirectory | Tested devices                                                                                                     |
|--------------|--------------------------------------------------------------------------------------------------------------------|
| android      | Meta Quest 2                                                                                                       |
| pc           | Windows (runs in a window, useful for debugging)                                                                   |
| pcvr         | OpenXR (SteamVR): Sony PlayStation VR2 (see https://www.playstation.com/en-us/support/hardware/pc-prepare-ps-vr2/) |

Actually, any headset with OpenXR support + Vulkan API is supposed to work.

First of all:
- You need to have [rust toolchain](https://rustup.rs/) installed.
- Clone this repo:
  ```
  git clone https://github.com/bandipapa/rsaber.git
  ```

The built-in demo song can be replaced by downloading songs from https://beatsaver.com/, and overwrite asset/song/demo.

### android

Prerequisite:
- Install Android Studio, then go to SDK Manager and install:
  - SDK Platforms -> Android 12L (API level 32)
  - SDK Tools -> NDK

- OpenXR Loader
  - Go to https://mvnrepository.com/artifact/org.khronos.openxr/openxr_loader_for_android
  - Download latest aar (which is actually a zip file), and extract jni/arm64-v8a/libopenxr_loader.so into android/lib/arm64-v8a/libopenxr_loader.so

- Setup rust toolchain, replace username and version as needed:
  ```
  rustup target add aarch64-linux-android
  cargo install cargo-apk
  set ANDROID_HOME=c:\Users\<username>\AppData\Local\Android\Sdk
  set ANDROID_NDK_ROOT=c:\Users\<username>\AppData\Local\Android\Sdk\ndk\<version>
  set PATH=%PATH%;c:\Program Files\Android\Android Studio\jbr\bin
  ```

Connect Quest to PC, then build:
```
cd android
cargo apk run
```

### pc

Build:
```
cd pc
cargo run
```

You can use keys w-a-s-d to move, z-x to change elevation, r to reset view and arrow keys to rotate camera.

### pcvr

Prerequisite:
- cmake is needed to build OpenXR loader, go to https://cmake.org/, and install it.

Build:
```
cd pcvr
cargo run
```

## Credits

- Demo song: Geoxor - Only Now

## Suggested Reading

- [Essence of linear algebra (excellent stuff, highly recommended)](https://www.youtube.com/watch?v=fNk_zzaMoSs&list=PLZHQObOWTQDPD3MizzM2xVFitgF8hE_ab)
- [Learn OpenGL (still relevant, even we use WebGPU)](https://learnopengl.com/)
- [WebGPU Fundamentals](https://webgpufundamentals.org/)
- [Learn Wgpu](https://sotrh.github.io/learn-wgpu/)
- [rust wgpu](https://docs.rs/wgpu/latest/wgpu/)
- [Normal Transformation](https://paroj.github.io/gltut/Illumination/Tut09%20Normal%20Transformation.html)

## TODO

- Saber<->note collision detection
- UI/menu system
- Linux port
- Put all assets into binary, but have the option to dump/read it from local file (this is for modders who don't want to recompile)
- Package crates properly, so they can be used with "cargo install"
