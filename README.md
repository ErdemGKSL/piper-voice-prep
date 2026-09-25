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

Menü: **Sesleri temizle ve böl → Otomatik metin üret → Kontrol → Piper metadata oluştur → Dil**. Varsayılan dil İngilizcedir; ana menüdeki **Language** satırına `Enter` basarak İngilizce ve Türkçe arasında geçebilirsiniz. Bu seçim hem arayüzü hem Whisper'ın tanıma dilini değiştirir. Whisper'ın çok dilli `base-q5_1` modeli çalıştırılabilir dosyanın içindedir; Python, `whisper-cli`, ayrı model dosyası veya internet bağlantısı gerekmez. Kontrol ekranında her klip sırayla otomatik çalınır. Ok tuşlarıyla gezilir; `Boşluk` tekrar çalar, `A` metinle eşleşiyorsa onaylar, `R` reddeder, `E` metni düzenler, `N` sıradaki bekleyen klibe gider. Düzenleme alanında imleçle gezme, `Home`/`End`, `Delete`, `Backspace`, `Shift` ile seçim, `Ctrl+A` ile tümünü seçme ve yapıştırma desteklenir. `Enter` kaydeder, `Esc` iptal eder; ardından `A` ile onaylanır. Her karar hemen `reviews.tsv` dosyasına yazılır.

Komut satırı otomasyonu da kullanılabilir. Linux:

```bash
./piper-voice-prep-linux prepare /path/to/voices
./piper-voice-prep-linux transcribe /path/to/voices --language en
./piper-voice-prep-linux finalize /path/to/voices
```

Windows PowerShell:

```powershell
.\piper-voice-prep-windows.exe prepare C:\path\to\voices
.\piper-voice-prep-windows.exe transcribe C:\path\to\voices --language en
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

`transcripts.tsv` dosyasında her WAV için bir satır bulunur: `dosya.wav<TAB>metin`. `reviews.tsv` kararları tutar. Metinleri dinleyip düzeltin, TUI'de onaylayın. Ardından metadata menüsünü veya `finalize` komutunu çalıştırın. Sadece **onaylı**, metni dolu ve WAV dosyası mevcut satırlar `metadata.csv` içine eklenir. `prepare` tekrar çalıştırıldığında mevcut metinler ve kararlar korunur; kaynak kayıtları değiştirdiyseniz ilgili klipleri yeniden kontrol edin. Kayıtlar farklı kişilerin sesini içeriyorsa klipleri elle ayıklayın; klasör adı dışında otomatik konuşmacı tanıma yapılmaz.

Piper'ın güncel `piper1-gpl` eğitimi için `--data.csv_path erdem/output/metadata.csv`, `--data.audio_dir erdem/output/wav`, `--model.sample_rate 22050` kullanın. Eski `rhasspy/piper` veri hazırlama akışı için `erdem/output` giriş dizini, `metadata_legacy.csv` dosyasını da `metadata.csv` adına kopyalayıp kullanın. İki formatın adlandırma farkı bu yüzden ayrı dosyalarda tutulur.

Metinsiz WAV parçaları **eğitime hazır değildir**. Yerleşik Whisper boş metinleri doldurur; zaten düzenlenmiş metinlere dokunmaz. Otomatik metinler hatalı olabilir, bu nedenle kontrol ekranında her birini dinleyip onaylayın. Reddedilen klipler eğitim metadata'sına girmez.

## İşleme

Symphonia ile çözümleme, 48 kHz üzerinde RNNoise gürültü azaltma, FFT tabanlı yeniden örnekleme, sessizlik ve ses seviyesine göre bölme. Varsayılan parça sınırları 1.5–12 saniye (`--min-seconds`, `--max-seconds`). Her parça tek tek tepe seviyesi korunarak normalize edilir. Çok sessiz, müzikli veya üst üste konuşmalı kayıtlar insan kontrolü gerektirir.

## Derleme

```bash
cargo build --release
```

Derlemede CMake ve C/C++ derleyicisi gerekir; Windows için `x86_64-pc-windows-gnu` hedefi ve MinGW-w64 araç zinciri kullanılır. `assets/ggml-base-q5_1.bin` kaynak projede saklanır ve derleme sırasında çalıştırılabilir dosyaya gömülür. Windows MinGW statik kütüphane adlandırması için `whisper-rs-sys` bağımlılığının küçük bir düzeltmesi `vendor/` altında bulunur. Kaynak kod MIT lisanslıdır; üçüncü taraf bilgileri `THIRD_PARTY.md` ve çalıştırılabilir dosyanın `licenses` komutundadır.

## Geliştirme sürümü

`main` dalına her push, GitHub Actions üzerinde Linux ve Windows için release binary'lerini derler ve UPX ile sıkıştırır. İki derleme de başarılı olunca `dev-main` etiketi son commit'e taşınır; aynı adlı ön sürümün iki indirilebilir dosyası yenilenir. GitHub deposunda Actions'ın çalışmasına ve `GITHUB_TOKEN` için `contents: write` iznine izin verilmelidir. Etiket veya release değişmez olarak korunuyorsa hareketli `dev-main` etiketi kullanılamaz.
