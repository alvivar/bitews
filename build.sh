#!/bin/bash

# Recreates a ./dist directory and copies the necessary files to run the server
# as a standalone.

cargo build --release
cd web/
npm run build
cd ..

rm -rf build
mkdir -p build
cp .env build/
cp target/release/chatserver.exe build/

mkdir -p build/web
cp web/index.html build/web/
cp web/style.css build/web/
cp web/favicon.ico build/web/

mkdir -p build/web/js
cp web/js/*.js build/web/js/
