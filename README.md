# rsaber: Beat Saber clone written in Rust

## State

We are really at the beginning. At least, the colored notes are already moving and there is a basic collision detection logic :).
As there are no built-in levels, they are fetched from https://beatsaver.com/.

Screenshots

<img src="https://raw.githubusercontent.com/bandipapa/rsaber/refs/tags/v0.6.0/doc/menu1.jpg" width="300" height="163">
<img src="https://raw.githubusercontent.com/bandipapa/rsaber/refs/tags/v0.6.0/doc/menu2.jpg" width="300" height="163">
<img src="https://raw.githubusercontent.com/bandipapa/rsaber/refs/tags/v0.6.0/doc/game1.jpg" width="300" height="163">
<img src="https://raw.githubusercontent.com/bandipapa/rsaber/refs/tags/v0.6.0/doc/game2.jpg" width="300" height="163">

## Supported Devices

<table>
  <tr>
    <th>Subdirectory</th>
    <th>Tested devices</th>
    <th><a href="https://github.com/bandipapa/rsaber/releases/">Pre-compiled binary</a></th>
  </tr>

  <tr>
    <td>android</td>
    <td>Meta Quest 2</td>
    <td>rsaber_android.apk</td>
  </tr>

  <tr>
    <td rowspan="2">pc (runs in a window, useful for debugging)</td>
    <td>Linux</td>
    <td>rsaber_pc_linux_x64</td>
  </tr>

  <tr>
    <td>Windows</td>
    <td>rsaber_pc_windows_x64.exe</td>
  </tr>

  <tr>
    <td>pcvr</td>
    <td>Windows (SteamVR/OpenXR): <a href="https://www.playstation.com/en-us/support/hardware/pc-prepare-ps-vr2/">Sony PlayStation VR2</a></td>
    <td>rsaber_pcvr_windows_x64.exe</td>
  </tr>
</table>

Actually, any headset with OpenXR support + Vulkan API is supposed to work.

## Supported Audio Backends

| OS      | Backend  |
|---------|----------|
| Android | AAudio   |
| Linux   | PipeWire |
| Windows | Wasapi   |

## Build From Source

If you prefer, you can compile rsaber from sources. First of all, you need to have [rust toolchain](https://rustup.rs/) installed.

### android

Prerequisite:
- Install Android Studio, then go to SDK Manager and install:
  - SDK Platforms -> API level: see ANDROID_SDK_LEVEL below
  - SDK Tools -> NDK

- Setup rust toolchain, replace username as needed:
  ```
  rustup target add aarch64-linux-android
  cargo install cargo-ndk
  set JAVA_HOME=c:\Program Files\Android\Android Studio\jbr
  set ANDROID_HOME=c:\Users\<username>\AppData\Local\Android\Sdk
  set ANDROID_SDK_LEVEL=34
  ```

Clone repo, connect Quest to PC, then build in debug mode & run:
```
git clone https://github.com/bandipapa/rsaber.git
cd rsaber\android\build
gradlew runDebug
```

Building in release mode can be done by setting ANDROID_KEYSTORE, ANDROID_KEYSTORE_PW, ANDROID_KEYALIAS, ANDROID_KEYALIAS_PW
environment variables, and execute:
```
gradlew runRelease
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

## Suggested Reading

- [Essence of linear algebra (excellent stuff, highly recommended)](https://www.youtube.com/watch?v=fNk_zzaMoSs&list=PLZHQObOWTQDPD3MizzM2xVFitgF8hE_ab)
- [Learn OpenGL (still relevant, even we use WebGPU)](https://learnopengl.com/)
- [WebGPU Fundamentals](https://webgpufundamentals.org/)
- [Learn Wgpu](https://sotrh.github.io/learn-wgpu/)
- [rust wgpu](https://docs.rs/wgpu/latest/wgpu/)
- [Normal Transformation](https://paroj.github.io/gltut/Illumination/Tut09%20Normal%20Transformation.html)

## TODO

- Have the option to dump/read assets from local files (this is for modders who don't want to recompile)
