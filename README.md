# Piper Voice Prep

Tek dosyalık Rust terminal uygulaması. Her birinci seviye klasörü ayrı bir konuşmacı sayar, alt klasörlerindeki sesleri tarar ve `output/wav` altında tek kanallı 22.05 kHz, 16 bit WAV parçaları üretir. Kaynak kayıtları değiştirmez.

## Kullanım

Uygulama, bir komut verilmediğinde menüyü açar. `voices` klasörüne konan çalıştırılabilir dosya için:

```bash
./piper-voice-prep-linux
```

```powershell
.\piper-voice-prep-windows.exe
```

Menü: Hazırla, Hazırla + Whisper, Kontrol, Piper metadata oluştur. Kontrol ekranında her klip sırayla otomatik çalınır. Ok tuşlarıyla gezilir; `Boşluk` tekrar çalar, `A` metinle eşleşiyorsa onaylar, `R` reddeder, `E` metni düzenler, `N` sıradaki bekleyen klibe gider. Düzenleme `Enter` ile kaydedilir, ardından `A` ile onaylanır. Her karar hemen `reviews.tsv` dosyasına yazılır.

Komut satırı otomasyonu da kullanılabilir. Linux:

```bash
./piper-voice-prep-linux prepare /path/to/voices
./piper-voice-prep-linux finalize /path/to/voices
```

Windows PowerShell:

```powershell
.\piper-voice-prep-windows.exe prepare C:\path\to\voices
.\piper-voice-prep-windows.exe finalize C:\path\to\voices
```

`voices/erdem`, `voices/elif` gibi her klasör ayrı ele alınır. Çıktı:

```text
erdem/output/
  wav/src001_clip00001.wav
  transcripts.tsv
  reviews.tsv
  metadata.csv
  metadata_legacy.csv
```

`transcripts.tsv` dosyasında her WAV için bir satır bulunur: `dosya.wav<TAB>metin`. `reviews.tsv` kararları tutar. Metinleri dinleyip düzeltin, TUI'de onaylayın. Ardından metadata menüsünü veya `finalize` komutunu çalıştırın. Sadece **onaylı**, metni dolu ve WAV dosyası mevcut satırlar `metadata.csv` içine eklenir. `prepare` tekrar çalıştırıldığında mevcut metinler ve kararlar korunur. Kayıtlar farklı kişilerin sesini içeriyorsa klipleri elle ayıklayın; klasör adı dışında otomatik konuşmacı tanıma yapılmaz.

Piper'ın güncel `piper1-gpl` eğitimi için `--data.csv_path erdem/output/metadata.csv`, `--data.audio_dir erdem/output/wav`, `--model.sample_rate 22050` kullanın. Eski `rhasspy/piper` veri hazırlama akışı için `erdem/output` giriş dizini, `metadata_legacy.csv` dosyasını da `metadata.csv` adına kopyalayıp kullanın. İki formatın adlandırma farkı bu yüzden ayrı dosyalarda tutulur.

Metinsiz WAV parçaları **eğitime hazır değildir**. İsteğe bağlı olarak [whisper.cpp](https://github.com/ggml-org/whisper.cpp) `whisper-cli` ve yerel model dosyası ile taslak transkripsiyon üretebilirsiniz:

```bash
./piper-voice-prep-linux prepare /path/to/voices --whisper-model /path/to/ggml-medium.bin --whisper-bin /path/to/whisper-cli --language tr
```

Whisper metinlerini eğitimden önce gözden geçirin. Model ve `whisper-cli` dosyası bu projenin parçası değildir. Parametre verilmezse uygulama tamamen çevrimdışı ve ek program gerektirmeden çalışır.

## İşleme

Symphonia ile çözümleme, 48 kHz üzerinde RNNoise gürültü azaltma, FFT tabanlı yeniden örnekleme, sessizlik ve ses seviyesine göre bölme. Varsayılan parça sınırları 1.5–12 saniye (`--min-seconds`, `--max-seconds`). Her parça tek tek tepe seviyesi korunarak normalize edilir. Çok sessiz, müzikli veya üst üste konuşmalı kayıtlar insan kontrolü gerektirir.

## Derleme

```bash
cargo build --release
```

Windows için `x86_64-pc-windows-gnu` hedefi ve uygun linker gerekir. Kaynak kod MIT lisanslıdır; bağımlılıkların lisansları Cargo paketlerinde yer alır.
