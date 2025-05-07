## Setup

Install espup and make sure it's sourced either by running:

```$HOME/export-esp.sh```

or for fish adding:

```sh
# Add Rust ESP toolchain path
fish_add_path "/Users/dc/.rustup/toolchains/esp/xtensa-esp-elf/esp-14.2.0_20240906/xtensa-esp-elf/bin"


check by running:
```sh
rustup toolchain list
# esp (active)
#


```

# Set LIBCLANG_PATH environment variable
set -gx LIBCLANG_PATH "/Users/dc/.rustup/toolchains/esp/xtensa-esp32-elf-clang/esp-19.1.2_20250225/esp-clang/lib"
```

Also make sure the esp32 is connected by the USB port on the device, and hold down the boot button if needed.
