# Omarchy Linux Application, Engine & Plugin Security Architecture Standards

Bu belge, Omarchy Linux ekosistemi için geliştirilen tüm yerel uygulamalarda, arka plan motorlarında (Rust/C++), eklentilerde (Quickshell/QML) ve Git entegrasyonlarında istisnasız ve kalıcı olarak uygulanmak zorunda olan resmi mimari güvenlik standartlarını ve geliştirici kılavuzunu tanımlar.

---

## 1. Alt Süreç (Subprocess) Yönetimi ve Mutlak Zaman Sınırı (Monotonic Deadlines)

1. **İzole Süreç Grupları (`cmd.process_group(0)`)**:
   - Başlatılan her alt süreç kendi bağımsız süreç grubunda (`cmd.process_group(0)`) çalıştırılmalıdır. Bu, alt sürecin kendisi veya çağırdığı yardımcıların ana süreçten bağımsız bir grup ID'sine (PGID = PID) sahip olmasını sağlar.
2. **Non-Blocking I/O (`O_NONBLOCK`) ve Bounded Polling**:
   - Alt süreçlerin standart çıktı (`stdout`) ve hata (`stderr`) boruları (`raw_fd`), `fcntl` ile anında `O_NONBLOCK` moduna alınmalıdır (`F_SETFL, flags | O_NONBLOCK`).
   - Borulardan okuma yapılırken asla blocking `Read::read()` çağrılmamalıdır. Bunun yerine POSIX `poll()` kullanılarak zaman dilimlerine bölünmüş (`min(50ms, remaining)`) okuma döngüsü kurulmalıdır.
   - Doğrudan alt süreç (`direct child`) sonlanmış olsa dahi `stdout` borusu hemen kapanmayabilir (arka planda boruyu açık tutan torun süreçler kalabilir). Bu nedenle tahliye (drain) döngüsü boyunca mutlak `Instant::now() >= deadline` denetimi aralıksız sürdürülmelidir.
3. **Koşulsuz Süreç Grubu Temizliği (`reap_process_group`)**:
   - Doğrudan alt sürecin çıkmış (`child.try_wait() == Ok(Some(_))`) olup olmadığına bakılmaksızın; zaman aşımı, bellek taşması (overrun), I/O hatası veya motorun sonlanması anında tüm süreç grubuna (`-pid`):
     - Önce `SIGTERM` (15) gönderilmelidir.
     - 5-10 ms bekleme süresi tanınmalıdır.
     - Ardından kaçınılmaz `SIGKILL` (9) gönderilerek `SIGTERM`'i yok sayan süreçler dahi imha edilmelidir.
     - Doğrudan süreç `child.wait()` ile zombi bırakılmadan toplanmalıdır.
   - Fonksiyon veya döngü beklenmedik şekilde terk edilirse (panic, erken return, teardown) süreçlerin yetim kalmaması için RAII `ProcessGroupGuard` kullanılmalıdır.
4. **Sınırlandırılmış Bellek (Bounded Buffer Caps)**:
   - Tüm boru okumaları için kesin boyut sınırları (örn. 64 KiB) konmalı; limit aşıldığı anda (`overrun`) süreç grubu öldürülüp işlem iptal edilmelidir.
5. **Temiz Ortam Değişkenleri (`env_clear`)**:
   - Alt süreçler temizlenmiş ortamda (`cmd.env_clear()`) ve yalnızca whitelist edilmiş değişkenlerle (`PATH=/usr/bin:/bin`, `LC_ALL=C`, `GIT_TERMINAL_PROMPT=0`) çalıştırılmalıdır.

---

## 2. Quickshell & QML Güvenlik Standartları (Desktop Shell UI)

1. **Düz Metin Güvencesi (`textFormat: Text.PlainText`)**:
   - Kullanıcıdan, sistemden, alt süreçlerden veya ağdan gelen dinamik verileri gösteren tüm QML `Text`, `TextEdit` ve etiket bileşenlerinde `textFormat: Text.PlainText` zorunludur. Zengin metin (HTML/CSS) ve betik enjeksiyonu kesinlikle engellenmelidir.
2. **Dinamik Kod Yürütmenin Yasaklanması (No Dynamic Eval)**:
   - `eval()`, `Qt.createQmlObject()` veya dinamik `Loader.source` yapıları kullanıcı/ağ girdileriyle asla beslenemez.
3. **Güvenli IPC Yönetimi (`Quickshell.Io.IpcHandler`)**:
   - `IpcHandler` fonksiyon parametreleri katı tiplerle (`string`, `int`, `bool`, `real`, `color`) açıkça tanımlanmalıdır.
   - IPC üzerinden gelen parametreler daima güvenilmeyen (untrusted) girdi olarak kabul edilmeli, aralık ve desen denetiminden geçirilmelidir.
   - Sistem kabuk komutları doğrudan IPC girdileriyle çalıştırılamaz.
4. **Arayüz Watchdog Zamanlayıcıları**:
   - QML tarafında arka plan motorları için bağımsız gözetim zamanlayıcıları (`Timer`) bulunmalı, kilitlenen veya zamanında yanıt vermeyen motorlar arayüz tarafından öldürülebilmelidir.
5. **Bileşen Yok Edilme Temizliği (`Component.onDestruction`)**:
   - Panel veya pencere kapandığında çalışan tüm alt süreçler ve zamanlayıcılar öldürülmeli ve toplanmalıdır.
6. **Vektör ve Varlık Güvenliği (SVG / Image Security)**:
   - Dış kaynaklı veya güvenilmeyen SVG dosyaları doğrudan ayrıştırılmamalı, güvenli yerel varlıklar (`preview.png`, sistem ikon temaları) tercih edilmelidir (CVE-2025-14576 vb. zafiyet önlemi).

---

## 3. Rust Güvenlik ve Geliştirici Standartları (Rust Secure Code WG)

1. **Bellek Güvenliği ve `unsafe` Disiplini**:
   - `unsafe` blokları kesin olarak en aza indirilmelidir. Kullanılan her `unsafe` bloğu, varsayılan güvenlik koşullarını (invariants) açıklayan açık bir `// SAFETY:` yorumu içermek zorundadır.
2. **Hata Yönetimi ve Paniklerin Engellenmesi**:
   - Üretim kodunda çıplak `.unwrap()` ve `.expect()` kullanımından kaçınılmalı, hatalar `Result` ve `?` operatörü ile açıkça ele alınmalıdır.
   - Tamsayı işlemlerinde taşmalara karşı güvenli veya doymuş aritmetik (`saturating_add`, `saturating_sub`, `checked_*`) kullanılmalıdır.
3. **Sertleştirme ve Bağımlılık Denetimi**:
   - Derleme uyarıları sıfırlanmalıdır (`cargo clippy --all-targets --all-features -- -D warnings`).
   - Bağımlılıklar `cargo-audit` ve RustSec Advisory Database ile bilinen güvenlik açıklarına karşı taranmalıdır.
4. **Çalışma Anında Değişebilir Kod Yürütmenin Yasaklanması (Zero Mutable Runtime Execution)**:
   - `npx -y <paket>`, `pip install` on-the-fly, `curl | sh`, dinamik `npm exec` gibi çalışma anında dış kaynaktan değişebilir kod indiren mekanizmalar kesinlikle YASAKTIR.
   - Gerekli tüm harici yardımcı araçlar (`cloudflared`, `localtunnel`, `qrencode`, vb.) sistemin güvenli `PATH` yollarında (`/usr/bin`, `/usr/local/bin`, `~/.local/bin`) önceden kurulu olmak zorundadır.
   - `README.md` içinde bu bağımlılıkların değişmez sürüm sabitlemeleri (`@2.0.2` vb.) ve resmi kurulum kaynakları eksiksiz belgelenmelidir.

---

## 4. Güvenli Dosya ve Anahtar Yönetimi (Mode 0600 & 0700)

1. **Umask'a Asla Güvenilmez**:
   - PIN kodları, oturum anahtarları, token'lar ve kimlik bilgileri `fs::write` ile doğrudan yazılamaz.
2. **Atomik ve Sadece Sahibine Özel İzinler (`0600`)**:
   - Hassas dosyalar aynı dizinde geçici bir dosya (`.tmp_...`) üzerinden umask'tan bağımsız olarak `OpenOptions::new().mode(0o600)` ve `set_permissions(0o600)` ile oluşturulmalı, veriler yazılıp diske senkronize edildikten (`sync_all`) sonra atomik `fs::rename()` ile taşınmalıdır.
   - Durum dizinleri (`~/.local/state/...`) ve indirme klasörleri `0700` (`drwx------`) ile kısıtlanmalıdır.
3. **Symlink ve Dosya Türü Doğrulaması**:
   - Hassas dosyalar okunmadan veya üzerine yazılmadan önce `symlink_metadata` denetlenmelidir:
     - Symlink'ler derhal reddedilmeli (`!meta.file_type().is_symlink()`), asla symlink takip edilmemelidir.
     - Normal dosya olmayan yapılar (FIFO, soket, aygıt, dizin) reddedilmelidir (`meta.file_type().is_file()`).
     - Dosya sahibinin mevcut çalışan kullanıcının UID'si olduğu (`meta.uid() == getuid()`) ve izinlerin kesinlikle `0600` olduğu (`meta.mode() & 0o777 == 0o600`) doğrulanmalıdır.

---

## 5. Git Güvenliği ve Komut Satırı Geliştirici Kılavuzu

1. **Argüman Enjeksiyonu Koruması (Argument Injection Prevention)**:
   - Git komutları asla bir kabuk dizesi (shell string) üzerinden çalıştırılamaz. Argümanlar ayrık dilimler (`&["status", "--porcelain=v1"]`) halinde verilmelidir.
   - Kullanıcı veya dizin girdilerinden gelen parametreler tire (`-`) ile başlıyorsa argüman olarak yorumlanması engellenmelidir.
   - Konumsal argümanları ve dosya yollarını bayraklardan ayırmak için daima `--` sınırlayıcısı (delimiter) kullanılmalıdır.
2. **Güvenli Dizin ve Ortam İzolasyonu**:
   - Git komutları daima `-C <path>` ile hedeflenen kanonik dizinde çalıştırılmalıdır.
   - `GIT_TERMINAL_PROMPT=0` atanarak etkileşimli kullanıcı istemleri kilitlenmeye yol açmadan engellenmelidir.
   - Güvenilmeyen depolarda özyinelemeli alt modül klonlaması (`git clone --recurse-submodules`) yapılmamalıdır.
3. **Git Takibinde Derlenmiş İkili Dosya Yasağı (Zero Prebuilt Binaries)**:
   - Derlenmiş motor ikilileri (`gitradar-engine`, `omasend-engine`, `sentinel-engine`, vb.) asla Git takibine eklenmemeli, `.gitignore` içine yazılmalıdır.
   - Uygulamalar `cargo build --release --locked` veya `PKGBUILD` üzerinden kaynaktan derlenmelidir.
   - Durum ve kontrol betikleri (`*-status`, `*-dashboard`), ikili dosya henüz derlenmemişse otomatik kaynaktan derleme geri dönüşüne (fallback) sahip olmalıdır.

---

## 6. Zorunlu Birim ve Regresyon Testleri

- Geliştirilen her güvenlik mekanizması `cargo test` altında otomatik birim testlerle doğrulanmalıdır:
  - Süreç zaman aşımında `SIGTERM`'i yok sayan ve `stdout`'u açık tutan torun süreçlerin dahi mutlak deadline içinde yok edildiğinin testi.
  - Hassas dosyaların `0600` izninin, symlink reddinin ve atomik yazımının testi.
  - Hatalı veya güvenilmeyen girdilerde panik yaşanmadan temiz `None` veya `Err` dönüldüğünün testi.

---

## 7. Geliştirici Desteği ve Fonlama Standartları (Funding & Sponsorship Architecture)

1. **Zorunlu GitHub Fonlama Yapılandırması (`.github/FUNDING.yml`)**:
   - Omarchy Linux ekosistemindeki tüm depolarda `.github/FUNDING.yml` dosyası istisnasız bulunmalıdır:
     ```yaml
     buy_me_a_coffee: ozdil
     custom: ['https://buymeacoffee.com/ozdil']
     ```
2. **README Rozet ve Destek Bölümü**:
   - Proje `README.md` başlığının hemen altına resmi Buy Me a Coffee rozeti eklenmelidir:
     ```markdown
     [![Buy Me A Coffee](https://img.shields.io/badge/Buy_Me_A_Coffee-Support_Development-FFDD00?style=for-the-badge&logo=buy-me-a-coffee&logoColor=black)](https://buymeacoffee.com/ozdil)
     ```
   - Lisans bölümünden önce resmi `Support & Sponsorship` başlığı ve buton görseli (`https://cdn.buymeacoffee.com/buttons/v2/default-yellow.png`) yer almalıdır.
3. **Masaüstü ve Web Arayüzü Buton Standartları**:
   - QML / Quickshell panellerinde veya web arayüzlerinde geliştiriciye destek butonu yer almalı; `#FFDD00` altın sarısı vurgu rengiyle `Qt.openUrlExternally("https://buymeacoffee.com/ozdil")` tetiklenmelidir.
