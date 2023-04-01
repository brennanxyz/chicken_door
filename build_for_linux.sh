rustup target add x86_64-unknown-linux-musl
brew install FiloSottile/musl-cross/musl-cross
brew install openssl

TARGET_CC=x86_64-linux-musl-gcc cargo build --release --target=x86_64-unknown-linux-musl