# Bancada de testes para Android

App mínimo que embute o programa `perdura` e o executa no aparelho. Serve para testes físicos. Não é o aplicativo final.

A identidade fica na pasta privada do app, ainda sem a proteção do Android Keystore.

## Compilar

Na raiz do repositório:

```sh
cargo ndk -t arm64-v8a build --release -p perdura-cli
mkdir -p android/app/src/main/jniLibs/arm64-v8a
cp target/aarch64-linux-android/release/perdura android/app/src/main/jniLibs/arm64-v8a/libperdura.so
cd android
gradle assembleDebug
```

Requer Android SDK (plataforma 36, build-tools 36.0.0), Android NDK e Gradle 9.6 ou mais novo.
