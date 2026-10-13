include("${CMAKE_CURRENT_LIST_DIR}/../../../res/vcpkg-triplets/arm64-android.cmake")

# AOM otherwise selects a host `as`, which is unavailable on Windows and cannot
# assemble Android arm64 code. Keep this host build setting out of the app paths.
if(CMAKE_HOST_WIN32)
  list(APPEND VCPKG_CMAKE_CONFIGURE_OPTIONS
    "-DCMAKE_ASM_COMPILER=$ENV{ANDROID_NDK_HOME}/toolchains/llvm/prebuilt/windows-x86_64/bin/clang.exe"
    "-DCMAKE_ASM_COMPILER_TARGET=aarch64-linux-android21")
endif()
