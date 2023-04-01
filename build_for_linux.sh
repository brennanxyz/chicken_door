# brew tap messense/macos-cross-toolchains
# brew install messense/macos-cross-toolchains
# brew install gcc
# brew install x86_64-unknown-linux-gnu
# brew install x86_64-linux-gnu-binutils
# brew install x86_64-linux-gnu-gcc
# brew install openssl

# export CC_X86_64_UNKNOWN_LINUX_GNU=x86_64-unknown-linux-gnu-gcc
# export CXX_X86_64_UNKNOWN_LINUX_GNU=x86_64-unknown-linux-gnu-g++
# export AR_X86_64_UNKNOWN_LINUX_GNU=x86_64-unknown-linux-gnu-ar
# export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=x86_64-unknown-linux-gnu-gcc

# rustup target add x86_64-unknown-linux-gnu

# TARGET_CC=x86_64-unknown-linux-gnu cargo build --target=x86_64-unknown-linux-gnu
#crate tree --target=x86_64-unknown-linux-gnu -i openssl-sys
rustup target add x86_64-unknown-linux-musl
brew install FiloSottile/musl-cross/musl-cross
brew install openssl

TARGET_CC=x86_64-linux-musl-gcc cargo build --release --target=x86_64-unknown-linux-musl