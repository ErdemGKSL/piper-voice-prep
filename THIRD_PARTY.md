# Third-party components

- Whisper model `assets/ggml-base-q5_1.bin`: OpenAI Whisper weights converted and quantized by the [whisper.cpp model repository](https://huggingface.co/ggerganov/whisper.cpp), MIT license. SHA-256: `422f1ae452ade6f30a004d7e5c6a43195e4433bc370bf23fac9cc591f01a8898`.
- [whisper.cpp](https://github.com/ggml-org/whisper.cpp) via `whisper-rs`: MIT license.
- [nnnoiseless](https://github.com/jneem/nnnoiseless): BSD-3-Clause license; derived from Xiph RNNoise.
- Other Rust dependencies: see `Cargo.lock` and their package license files.

The Whisper model and its inference code are included in the release executables. The source project keeps the model file to make release builds reproducible.
