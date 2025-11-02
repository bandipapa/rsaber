# rsaber: Beat Saber clone written in Rust

## State

We are really at the beginning. At least, the colored notes are already moving and there is a basic collision detection logic :).

## Supported Devices

| Subdirectory | Tested devices                                                                                                  |
|--------------|-----------------------------------------------------------------------------------------------------------------|
| android      | Meta Quest 2                                                                                                    |
| pc           | Windows (runs in a window, useful for debugging)                                                                |
| pcvr         | OpenXR (SteamVR): [Sony PlayStation VR2](https://www.playstation.com/en-us/support/hardware/pc-prepare-ps-vr2/) |

Actually, any headset with OpenXR support + Vulkan API is supposed to work.

## Download

You can go to [releases](https://github.com/bandipapa/rsaber/releases/) to download pre-compiled binaries.

## Build From Source

If you prefer, you can compile rsaber from sources. First of all, you need to have [rust toolchain](https://rustup.rs/) installed.

### android

Prerequisite:
- Install Android Studio, then go to SDK Manager and install:
  - SDK Platforms -> Android 12L (API level 32)
  - SDK Tools -> NDK

- Setup rust toolchain, replace username and version as needed:
  ```
  rustup target add aarch64-linux-android
  cargo install cargo-apk
  set ANDROID_HOME=c:\Users\<username>\AppData\Local\Android\Sdk
  set ANDROID_NDK_ROOT=c:\Users\<username>\AppData\Local\Android\Sdk\ndk\<version>
  set PATH=%PATH%;c:\Program Files\Android\Android Studio\jbr\bin
  ```

In the past, manual downloading of OpenXR Loader was needed, but it has been integrated in the build
process already.

Clone repo, connect Quest to PC, then build & run:
```
git clone https://github.com/bandipapa/rsaber.git
cd rsaber\android
cargo apk run
```

### pc

Build & run:
```
cargo install rsaber_pc
rsaber_pc
```

### pcvr

Prerequisite:
- cmake is needed to build OpenXR loader, go to https://cmake.org/, and install it.

Build & run:
```
cargo install rsaber_pcvr
rsaber_pcvr
```

## Credits

- Demo level: Geoxor - Only Now

## Suggested Reading

- [Essence of linear algebra (excellent stuff, highly recommended)](https://www.youtube.com/watch?v=fNk_zzaMoSs&list=PLZHQObOWTQDPD3MizzM2xVFitgF8hE_ab)
- [Learn OpenGL (still relevant, even we use WebGPU)](https://learnopengl.com/)
- [WebGPU Fundamentals](https://webgpufundamentals.org/)
- [Learn Wgpu](https://sotrh.github.io/learn-wgpu/)
- [rust wgpu](https://docs.rs/wgpu/latest/wgpu/)
- [Normal Transformation](https://paroj.github.io/gltut/Illumination/Tut09%20Normal%20Transformation.html)

## TODO

- Linux port
- Have the option to dump/read assets from local files (this is for modders who don't want to recompile)
