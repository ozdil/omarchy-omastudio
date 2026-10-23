# OmaStudio Kullanım Kılavuzu

**OmaStudio: The Complete Medium Format RAW Editor for Omarchy**

[Tüm diller](../README.md) · [Teknolojiler](../technology/tr.md) · [README](../../README.tr.md)

Bu kılavuz OmaStudio 3.1 içindir. Uygulamanın nasıl kurulacağını, pencerede nelerin nerede olduğunu ve her panelin ve aracın nasıl kullanılacağını anlatır. Ekran görüntülerinde [raw.pixls.us](https://raw.pixls.us) sitesinden alınan Fujifilm GFX 50R ve Hasselblad CFV-50c örnek dosyaları (CC0) kullanılmıştır.

![Fujifilm GFX 50R fotoğrafıyla OmaStudio](../images/lang/main-tr.webp)

## İçindekiler

1. [Kurulum](#1-kurulum)
2. [Pencere](#2-pencere)
3. [Fotoğraf açma ve kitaplık](#3-fotoğraf-açma-ve-kitaplık)
4. [Temel ve Pro](#4-temel-ve-pro)
5. [Ayarla sekmesi](#5-ayarla-sekmesi)
6. [Maskeler sekmesi](#6-maskeler-sekmesi)
7. [Görünümler sekmesi](#7-görünümler-sekmesi)
8. [Yapay zekâ sekmesi ve YZ sohbeti](#8-yapay-zekâ-sekmesi-ve-yz-sohbeti)
9. [Görüntüleme yardımcıları](#9-görüntüleme-yardımcıları)
10. [Geçmiş, anlık görüntüler ve sürümler](#10-geçmiş-anlık-görüntüler-ve-sürümler)
11. [Fujifilm](#11-fujifilm)
12. [Orta format](#12-orta-format)
13. [Dışa aktarma](#13-dışa-aktarma)
14. [Bulut hesapları](#14-bulut-hesapları)
15. [Klavye ve fare](#15-klavye-ve-fare)
16. [Komut satırı, denetim soketi ve MCP](#16-komut-satırı-denetim-soketi-ve-mcp)
17. [Dosyalar ve klasörler](#17-dosyalar-ve-klasörler)
18. [Sorun giderme](#18-sorun-giderme)

## 1. Kurulum

OmaStudio, Wayland ve Hyprland altında Omarchy (Arch Linux) üzerinde çalışır.

**Paketten.** Deponun bir kopyasında:

```bash
makepkg -si
```

Bu komut `omastudio`, `omastudio-app`, `omastudio-engine`, ICC profillerini, masaüstü girdisini, simgeleri ve USB'ye bağlı Fujifilm fotoğraf makineleri için bir udev kuralını kurar.

**Sürüm arşivinden.** [Sürümler sayfasından](https://github.com/ozdil/omarchy-omastudio/releases) `omastudio-<sürüm>-x86_64-linux.tar.gz` dosyasını indirin, `checksums.txt` ile doğrulayın, açın ve `bin/*` içeriğini `~/.local/bin` klasörüne, `share/omastudio` klasörünü `~/.local/share/omastudio` konumuna kopyalayın.

**Gerekli paketler:** `libraw` 0.22 veya üstü, `lcms2`, 6.12 veya üstü `qt6-base`, `qt6-declarative`, `qt6-wayland` ve `qt6-svg`, dosya pencereleri için `zenity`.

**İsteğe bağlı paketler:**

| Paket | Kullanım amacı |
| :--- | :--- |
| `vulkan-icd-loader` ve ekran kartınızın Vulkan sürücüsü | GPU'da demozaikleme |
| `perl-image-exiftool` | Dışa aktarılan dosyalarda EXIF bilgilerini korumak |
| `libwebp-utils`, `libheif`, `ffmpeg` | Kayıplı WebP, HEIF ve AVIF yedeği |
| `onnxruntime` | NPU, GPU veya CPU üzerinde yerel YZ maskeleri |
| `rclone`, `libsecret`, `gnome-keyring` | Google Drive, OneDrive, Dropbox, Box, pCloud ve Yandex Disk |
| `github-cli` | GitHub hesabı |
| `ollama` | Düzenleme asistanı için yerel YZ modelleri |
| `quickshell` | Omarchy çubuğundaki OmaStudio düğmesi |

OmaStudio'yu Omarchy uygulama menüsünden başlatın ya da `omastudio` komutunu çalıştırın (isterseniz bir fotoğrafla: `omastudio DSCF0001.RAF`). Açık bir pencere varsa fotoğraf o pencerede açılır.

## 2. Pencere

| Alan | İçerik |
| :--- | :--- |
| Üst çubuk | Dosya adı, işaretler ve yıldız puanı, **Yerel YZ'ye bağlan**, **Temel** ve **Pro** anahtarı, geri al ve yinele, önce ve sonra (**A \| B**), **Kırp**, dil menüsü, Hakkında, **RAW aç** ve **Dışa aktar** |
| Sol panel | Gezgin, sırpma göstergeli ölçüm araçları (histogram, dalga formu, RGB geçidi, vektörskop), fotoğraf makinesi ve objektif bilgileri, önerilen sahne, geçmiş ve anlık görüntüler |
| Orta | Fotoğraf, üstünde sürüm çubuğu (A–D), altında görünüm çubuğu: döndür, **Sığdır**, %100, %200, geçerli yakınlaştırma, çevre rengi, **Büyüteç**, **Odak**, **Karşılaştır** |
| Sağ panel | **Ayarla**, **Maskeler** (Pro), **Görünümler** ve **Yapay zekâ** sekmeleri ile **Kopyala**, **Yapıştır**, **Sıfırla**, **XMP içe aktar** ve **XMP yaz** düğmelerinin bulunduğu sabit bir satır |
| Film şeridi | **Klasörler**, **Katalog**, her bulut hesabı için bir sekme, **Bağlan**, **Yukarı** ve geçerli klasördeki fotoğraflar |

**Tab** iki yan paneli ve film şeridini birlikte gizler ve gösterir. Her panel bölümü başlığına tıklanarak açılıp kapanır; bir başlığa **Ctrl+tık** yalnızca o bölümü açık bırakır. Açık sekme, kapalı bölümler ve hangi panellerin gösterildiği oturumlar arasında hatırlanır.

Renkler, yazı tipi ve köşe yuvarlaklığı etkin Omarchy temasını canlı olarak izler. Fotoğrafın çevresi, temanın renk algısını değiştirmemesi için varsayılan olarak nötr koyu gridir; görünüm çubuğundaki çevre düğmesi orta griye veya tema rengine geçirir.

| Tokyo Night | Catppuccin Latte | Gruvbox | Rosé Pine |
| :---: | :---: | :---: | :---: |
| ![](../images/main-essentials.webp) | ![](../images/theme-catppuccin-latte.webp) | ![](../images/theme-gruvbox.webp) | ![](../images/theme-rose-pine.webp) |

Arayüz 30 dilde kullanılabilir. Sistem dilini izler; üst çubuktaki dil menüsünden başka bir dil seçilebilir ve hemen uygulanır.

## 3. Fotoğraf açma ve kitaplık

- **RAW aç** (Ctrl+O) dosya penceresini açar. Fotoğraflar, RAW veya Görüntüler olarak süzer.
- Dosya yöneticisinden bir fotoğrafı veya klasörü pencereye sürükleyin.
- Film şeridinde **Klasörler** geçerli klasörü listeler; girmek için bir klasöre, geri dönmek için **Yukarı** düğmesine tıklayın. Açmak için bir fotoğrafa tıklayın. Ctrl+← ve Ctrl+→ önceki ve sonraki fotoğrafa geçer.
- **Katalog**, OmaStudio'nun gördüğü her fotoğrafı puanı, işareti ve renk etiketiyle listeler.

OmaStudio, LibRaw 0.22'nin okuduğu her RAW biçimini (Fujifilm, Hasselblad, Phase One, Leica, Pentax, Mamiya/Leaf, Canon, Nikon, Sony, Panasonic, Olympus, Sigma, DNG ve diğerleri) ve JPEG, PNG, WebP, TIFF, HEIF/HEIC, AVIF, JPEG XL, OpenEXR, Radiance HDR ve PNM dosyalarını açar.

**Puan ve işaretler.** 1–5 yıldız puanını verir, 0 siler. P seçildi, X reddedildi olarak işaretler, U işareti kaldırır. Puanlar ve işaretler hemen kataloğa ve yan dosyaya kaydedilir.

**Seçim.** Film şeridinde Shift+tık bir aralık seçer, Ctrl+tık bir fotoğrafı ekler veya çıkarır. Birden çok fotoğraf seçiliyken Dışa aktar penceresi hepsini dışa aktarır (**Toplu**).

Özgün dosyalarınız hiçbir zaman değiştirilmez. Düzenlemeler çalışırken fotoğrafın yanındaki bir yan dosyaya, `<fotoğraf>.omastudio`, kaydedilir (Ctrl+S hemen kaydeder).

## 4. Temel ve Pro

Üst çubuktaki anahtar kaç aracın gösterileceğini seçer.

- **Temel:** profil, Fujifilm, beyaz dengesi, ton, canlılık ve doku, hazır ayarlar ve 3B LUT'lar.
- **Pro** bunlara ton eğrisi, renk karıştırıcı, renk derecelendirme, ayrıntı ve optik, dönüştürme, gren, renk yönetimi ve **Maskeler** sekmesini ekler. Pro bölümlerinde "Pro" etiketi bulunur.

Temel'e geri dönmek Pro bölümlerini gizler; ayarları uygulanmış olarak kalır.

![Ton eğrisiyle Pro modu](../images/pro-curve.webp)

## 5. Ayarla sekmesi

Her kaydırıcı çift tıklandığında sıfırlanır. Bölüm başlığındaki sıfırlama düğmesi tüm bölümü sıfırlar.

### Profil

Temel görünüm: **Standart / Kapalı**, Fujifilm film simülasyonları (Provia, Velvia, Astia, Classic Chrome, Classic Neg, Eterna, Eterna Bleach Bypass, Reala Ace, Nostalgic Neg, PRO Neg Std ve Hi, sarı, kırmızı ve yeşil filtreli Acros ve Monochrome, Sepia) ve Hasselblad tarzı doğal renk profili. Sekmeler listeyi süzer. **Miktar** profili nötr görüntüyle karıştırır.

### Fujifilm

**Color Chrome efekti** ve **Color Chrome FX Mavi** (Zayıf = 50, Güçlü = 100) doygun renkleri, FX Mavi ile maviyi, tonlarını koruyarak derinleştirir. Tarif kitaplığı, USB'deki fotoğraf makinesi ve Pixel Shift [Fujifilm](#11-fujifilm) bölümünde anlatılır.

### Beyaz dengesi

Fotoğraf makinesinin kaydettiği beyaz dengesine göre **Sıcaklık** (2000–12000 K) ve **Ton**. Fotoğraf "çekildiği gibi" açılır.

### Ton

**Pozlama** (doğrusal ışık üzerinde −5 ile +5 EV arası: +1 EV ışığı iki katına çıkarır), **Kontrast**, **Vurgular**, **Gölgeler**, **Beyazlar** ve **Siyahlar**.

### Canlılık ve doku

**Doku**, **Netlik** (orta ton kontrastı), **Pus giderme**, **Canlılık** ve **Doygunluk**.

### Ton eğrisi (Pro)

Histogram üzerinde RGB için ve kırmızı, yeşil, mavi için ayrı ayrı nokta eğrisi. Nokta eklemek için tıklayın, taşımak için sürükleyin, silmek için çift tıklayın. Eğri hiçbir zaman noktalarının dışına taşmaz. Alttaki dört bölge kaydırıcısı (Vurgular, Açık tonlar, Koyu tonlar, Gölgeler) eğriyi alana göre biçimlendirir.

### Renk karıştırıcı (Pro)

Sekiz renk için ton, doygunluk ve parlaklık: kırmızı, turuncu, sarı, yeşil, camgöbeği, mavi, mor ve eflatun.

![Renk karıştırıcı](../images/pro-hsl.webp)

### Renk derecelendirme (Pro)

Gölgeler (lift), orta tonlar (gamma) ve vurgular (gain) için üç yönlü renk tekerlekleri ve genel bir ofset; dört tekerlek ya da sekmeler halinde. **Renk güçlendirme**, **Orta ton detayı** ve **Kontrast pivotu** tekerleklerin altındadır. Onların altındaki kanal karıştırıcı kırmızı, yeşil ve maviyi her çıkış kanalına ya da tek renkli bir karışıma katar.

![Renk derecelendirme](../images/pro-grading.webp)

### Ayrıntı ve optik (Pro)

**Keskinleştirme**, **Gürültü azaltma** (parlaklık ve renk), **Vinyet**, **Renk saçağı giderme** (kromatik sapma) ve **Objektif bozulması düzeltme**. Dosyadaki Fujifilm ve DNG objektif düzeltmeleri otomatik uygulanır; bu kaydırıcılar onların üzerine çalışır.

### Dönüştür (Pro)

**Dikey**, aşağıdan çekilmiş bir bina gibi birbirine yaklaşan çizgileri düzeltir; **Yatay** aynısını yanlara doğru yapar; **Ölçek** sonucu büyütür. Boş kenarları kaldırmak için ardından kırpın.

### Gren (Pro)

Film greninin **Miktar**, **Boyut** ve **Pürüzlülük** değerleri.

### Renk yönetimi (Pro)

- **Gam sıkıştırma**, gamın dışına itilen renkleri kırpmak yerine yumuşakça geri getirir.
- **ACES ton eğrisi**, ACES çıkış dönüşümüne uydurulmuş bir yaklaşım olan filmik bir vurgu yumuşatmasıdır.
- **Ekran provası** (S), fotoğrafı bir çıkış profiliyle, örneğin bir yazıcı ve kâğıt profiliyle, nasıl görüneceği gibi gösterir. Profili ve oluşturma amacını seçin. **Gamut uyarısı**, profilin üretemediği renkleri eflatuna boyar. Düzenleme ve dışa aktarma değişmez.

Çıkış renk alanı Dışa aktar penceresinde seçilir.

![Renk yönetimi ve ekran provası](../images/soft-proof.webp)

## 6. Maskeler sekmesi

Maskeler sekmesi Pro modunda gösterilir.

### Rötuş ve klon

**Noktaları düzenle** seçeneğini açın ve kaldırmak için fotoğraftaki bir lekeye tıklayın. Leke yanındaki bir kaynaktan doldurulur. Lekeyi taşımak için düz çemberi, başka bir kaynak seçmek için kesikli çemberi sürükleyin. **Rötuş** kopyalanan alanı lekenin çevresindeki ışık ve renge uydurur; **Klon** olduğu gibi kopyalar. **Boyut**, **Yumuşak kenar** ve **Opaklık** seçili lekeye ve yenilerine uygulanır.

![Rötuş ve klon](../images/heal-clone.webp)

### Yerel ayarlamalar

Her katman pozlama, kontrast, vurgular, gölgeler, beyazlar, siyahlar ve doygunluğu bir maske üzerinden, bir opaklıkla uygular.

| Maske | Nasıl oluşturulur |
| :--- | :--- |
| **Doğrusal** | Fotoğraf üzerindeki iki tutamaç arasında bir geçiş |
| **Dairesel** | Merkez ve kenar tutamaçları olan, yumuşak kenarlı bir elips |
| **Parlaklık** | Bir parlaklık aralığı (en koyu, en parlak, geçiş) |
| **Renk** | Fotoğraftan seçilen bir renge yakın renkler (aralık ve yumuşak kenar) |
| **Fırça** | Fotoğrafa boyanır; boyut, yumuşak kenar, akış ve silgi modu vardır; orta fare düğmesi kaydırır |
| **YZ Özne**, **YZ Gökyüzü** | Bilgisayarınızdaki bir bölütleme modeli (bkz. [Yapay zekâ](#8-yapay-zekâ-sekmesi-ve-yz-sohbeti)) |

**Birleştir:** altında katmanın maskesine başka parçalar eklenebilir, çıkarılabilir veya kesiştirilebilir; her parça ters çevrilebilir.

![Yerel ayarlamalar](../images/masks.webp)

## 7. Görünümler sekmesi

- **Hazır ayarlar:** **Otomatik** fotoğrafı inceler ve pozlamayı, beyaz dengesini ve tonu ayarlar. Sönük, Canlı, Sıcak portre, Siyah beyaz ve Turkuaz ve turuncu görünümleri öncekine eklenmek yerine onun yerine geçer; beyaz dengesi, pozlama, kırpma ve film simülasyonu korunur.
- **LUT:** **.cube dosyası yükle…** kendi 3B LUT'unuzu bir miktarla uygular.

![Görünümler](../images/looks.webp)

## 8. Yapay zekâ sekmesi ve YZ sohbeti

Üst çubuktaki **Yerel YZ'ye bağlan**, bu bilgisayarda kurulu YZ'yi kullanır: Omarchy'nin varsayılan ajanı (kendi oturum açmalarıyla Claude Code veya Codex), yoksa Ollama, o da yoksa LM Studio. Bağlandıktan sonra düğme fotoğrafın üzerinde **YZ sohbeti**ni açar: neyi değiştirmek istediğinizi yazın ("daha sıcak, gökyüzünü kurtar"), YZ ayarları yapsın. Ctrl+Z her düzenlemeyi geri alır.

Yapay zekâ sekmesindeki **Düzenleme asistanı** aynı işi panelden yapar ve bunun yerine bir bulut sağlayıcısı kullanabilir: Anthropic Claude, OpenAI, OpenRouter veya Google Gemini. API anahtarları sistem anahtarlığında saklanır, hiçbir zaman diske yazılmaz veya komut satırına konmaz. Bir bulut sağlayıcısıyla ya da Claude Code veya Codex ile fotoğrafın önizlemesi o hizmete gönderilir; Ollama veya LM Studio ile bilgisayarınızda kalır.

![Düzenleme asistanı](../images/ai-assistant.webp)

**YZ maskeleri.** Maskeler sekmesindeki **YZ Özne** ve **YZ Gökyüzü** bu bilgisayarda bir ONNX bölütleme modeli çalıştırır: önce bir NPU'da (OpenVINO ile Intel Core Ultra, QNN ile Snapdragon X, Vitis AI ile AMD Ryzen AI), sonra bir GPU'da, sonra CPU'da. Hiçbir model paketlenmez veya indirilmez. Bir modeli `~/.local/share/omastudio/models` klasörüne `subject.onnx` veya `sky.onnx` adıyla koyun; `omastudio npu` nelerin kurulu olduğunu gösterir.

## 9. Görüntüleme yardımcıları

| Yardımcı | Nasıl kullanılır |
| :--- | :--- |
| Yakınlaştırma | İmleç konumunda fare tekerleği; **Sığdır** (Ctrl+0), **%100** (Ctrl+1), **%200**; çift tık sığdır ile %200 arasında geçiş yapar. %100, her ekran ölçeğinde bir ekran pikseline bir fotoğraf pikseli gösterir. |
| Kaydırma | Sol veya orta fare düğmesiyle sürükleyin; Shift+tekerlek yana kaydırır |
| Döndürme | Görünüm çubuğundaki döndürme düğmeleri; Alt+tekerlek 1,5°'lik adımlarla döndürür |
| Kırp (C) | En-boy hazır ayarları (Özgün, 1:1, 4:5, 9:16, 16:9, Serbest) ve ızgaralar (üçte bir, altın oran, altın sarmal) |
| Önce ve sonra (Y veya A \| B) | Fotoğrafı özgün ve düzenlenmiş olarak ikiye böler |
| Karşılaştır (Shift+C) | Düzenlemeyi başka bir durumun yanında gösterir: Önce, bir sürüm veya bir anlık görüntü |
| Büyüteç (L) | İmlecin altında tam çözünürlüklü fotoğrafı 1:1 gösteren bir kare |
| Odak vurgusu (Shift+F) | En keskin kenarları yeşile boyar; yalnızca görüntüleme yardımcısıdır |
| Sırpma | Kırpılan gölgeleri mavi, vurguları kırmızı göstermek için histogramın üstündeki **[S]** veya **[H]** düğmesine tıklayın |

| Odak vurgusu | Karşılaştır |
| :---: | :---: |
| ![](../images/focus-peaking.webp) | ![](../images/compare.webp) |

| Kırp | Önce ve sonra |
| :---: | :---: |
| ![](../images/crop.webp) | ![](../images/split.webp) |

Fotoğraf önizlemeden büyük gösterildiğinde motor görünen alanı tam çözünürlüklü dosyadan işler; böylece gördüğünüz ayrıntı dışa aktarılanla aynıdır.

## 10. Geçmiş, anlık görüntüler ve sürümler

- Sol paneldeki **Geçmiş** düzenlemenin her adımını adıyla listeler ("Exposure +0.70", "Tone curve"). Bir adıma geri dönmek için tıklayın; oradan yapılan yeni bir düzenleme, Ctrl+Z'de olduğu gibi sonraki adımların yerine geçer.
- **Anlık görüntüler** geçerli düzenlemeyi bir adla saklar. **Geri yükle** birini Ctrl+Z ile geri alınabilen yeni bir adım olarak geri getirir. Anlık görüntüler yan dosyaya kaydedilir.
- Fotoğrafın üstündeki **A–D sürümleri** aynı fotoğrafın en fazla dört ayrı düzenlemesini tutar. Geçmek için bir harfe tıklayın (Alt+1 ile Alt+4); sağ tık geçerli düzenlemeyi o sürüme kopyalar.
- **Kopyala** ve **Yapıştır** (Ctrl+Shift+C, Ctrl+Shift+V) bir düzenlemeyi başka bir fotoğrafa taşır. **Sıfırla** (Ctrl+R) varsayılanlara döner.
- **XMP yaz** düzenlemeyi, puanı ve etiketi Lightroom, Capture One, darktable ve digiKam'ın okuduğu bir `.xmp` yan dosyası olarak kaydeder; **XMP içe aktar** bu programların yazdığı bir dosyayı okur.

## 11. Fujifilm

- **Fotoğraf makinesinin tarifi.** Bir RAF veya Fujifilm JPEG dosyası çekildiği ayarlarla açılır: film simülasyonu, ton, renk, keskinlik, gürültü azaltma, netlik, gren, Color Chrome, beyaz dengesi kayması ve dinamik aralık (D-Range Priority dahil). Kaydedilmiş bir yan dosya önceliklidir.
- **Parlaklık.** RAF dosyaları fotoğraf makinesinin kendi JPEG'inin parlaklığında açılır.
- **Objektif düzeltmeleri.** XF, XC ve GF objektifleri için fotoğraf makinesinin kaydettiği bozulma, yanal renk sapması ve vinyet uygulanır.
- **Tarif kitaplığı.** **İçe aktar** bir X RAW Studio profilini (.FP1, .FP2, .FP3), tarif metnini ("Film Simulation: Classic Chrome / Grain Effect: Strong, Small …") veya bir Fujifilm fotoğrafını alır. **Tarifi uygula** kaydedilmiş bir tarifi uygular; **Fotoğrafın tarifini uygula** dosyadakine geri döner; **Kaydet** geçerli tarifi saklar.
- **USB üzerinden fotoğraf makinesi.** Fotoğraf makinesinin USB modunu USB RAW CONV./BACKUP RESTORE yapın ve bağlayın. **C1–C7'yi oku** özel ayarlarını gösterir, **Tarifi C…'e yaz** bir tarifi bir yuvaya kaydeder (oradakinin yerine geçer), **Fotoğraf makinesinde dönüştür** açık RAF'ı fotoğraf makinesinde, isterseniz seçili tarifle, işler.
- **Pixel Shift.** Film şeridinde bir GFX Pixel Shift dizisinin RAF karelerini seçin ve **Kareleri DNG olarak birleştir** düğmesine tıklayın. Dört kare her pikselde tam renk verir; on altı kare çözünürlüğü iki katına çıkarır.

## 12. Orta format

- **Fujifilm GFX** (GFX 50S, 50R, 50S II, 100, 100S, 100 II ve diğerleri): algılayıcıdan dışa aktarmaya 16 bit işleme, film simülasyonları, objektif düzeltmeleri ve Pixel Shift.
- **Hasselblad** 3FR ve FFF dosyaları fotoğraf makinesinin varsayılan kırpmasına göre kırpılır.
- **Phase One ve Leaf** IIQ ve MOS dosyaları arkalığın kendi algılayıcı kalibrasyonunu alır.
- **Mamiya** MEF ve **Pentax 645** PEF dosyaları desteklenir.
- **Düz alan (LCC):** `omastudio-engine medium-format lcc-create <raw>` eşit aydınlatılmış beyaz bir hedefin çekiminden bir düzeltme oluşturur; bu düzeltme o klasördeki fotoğraflara uygulanır.
- **Çoklu çekim:** `medium-format multishot f1 f2 f3 f4 --out merged.tif` 4 çekimlik bir diziyi birleştirir.

![Bir Hasselblad CFV-50c dosyası](../images/hasselblad.webp)

## 13. Dışa aktarma

**Dışa aktar** (E) Dışa aktar penceresini açar. Enter dışa aktarır, Esc kapatır.

![Dışa aktar penceresi](../images/export.webp)

| Ayar | Seçenekler |
| :--- | :--- |
| Biçim | JPEG, PNG (8/16 bit), TIFF (8/16 bit; sıkıştırmasız, LZW veya ZIP), WebP, AVIF (8/10/12 bit), HEIF (8/10/12 bit), JPEG XL (8/16 bit) ve OpenEXR (yarım veya tam kayan nokta, sahne doğrusal) |
| Kalite | 1–100; 100, WebP, AVIF ve JPEG XL için kayıpsızdır |
| Boyut | Tam, %75, %50 ya da 3840 veya 2048 piksel uzun kenar |
| Renk profili | sRGB, Display P3, Adobe RGB, ProPhoto RGB, Rec. 2020, ACEScg veya kendi ICC profiliniz |
| Oluşturma amacı | Algısal, Göreli (varsayılan), Doygunluk, Mutlak |
| Meta veriler | Fotoğraf makinesi bilgilerini (EXIF) koru, konumu (GPS) kaldır |
| Hazırla: | Instagram, Hikâyeler ve Reels, X, Facebook, kare ve YouTube küçük resimleri için kırpma ve çıkış keskinleştirmesi |
| Filigran | EXIF etiketli metin ({camera}, {lens}, {iso}, {aperture}, {shutter}, {focal}) veya konum, opaklık, boyut ve gölgeli bir PNG logo |
| Klasör ve dosya adı | Varsayılan `~/Pictures/OmaStudio_Exports`; dosya adına bir etiket eklenebilir |
| Yükleme hedefi | Bağlı bir bulut hesabı |

OpenEXR dışındaki her biçim kendi renk alanının ICC profilini taşır. Toplu dışa aktarma seçili fotoğrafları birbiri ardına dışa aktarır.

## 14. Bulut hesapları

Film şeridindeki **Bağlan** Google Drive, OneDrive, Dropbox, Box, pCloud, Yandex Disk veya GitHub ekler. Tarayıcınızda hizmetin kendi sayfasında oturum açarsınız; OmaStudio parolanızı hiçbir zaman görmez.

![Bulut hesapları](../images/cloud.webp)

- Her hesap film şeridinde bir sekme alır. Klasörlerinde gezinin ve bir fotoğrafı indirip (bir kez) açmak için tıklayın.
- Her hesapta `OmaStudio` adlı bir klasör oluşturulur (GitHub'da `OmaStudio` adlı özel bir depo) ve Dışa aktar penceresinden yüklemeleri alır.
- Depolama hizmetleri, sistem anahtarlığında saklanan bir anahtarla şifrelenmiş OmaStudio'ya ait yapılandırmayla rclone üzerinden çalışır. GitHub, GitHub CLI üzerinden çalışır.
- Google Drive için Google Cloud Console'dan aldığınız bir istemci kimliği **Kendi Google istemci kimliğiniz** altına girilebilir.
- **Bağlantıyı kes** hesabın anahtarlarını ve indirilen kopyaları bu bilgisayardan siler; hesaptaki dosyalar kalır.

## 15. Klavye ve fare

| Tuş | İşlem |
| :--- | :--- |
| Ctrl+O | Fotoğraf aç |
| Ctrl+S | Düzenlemeyi şimdi kaydet |
| Ctrl+Z / Ctrl+Shift+Z veya Ctrl+Y | Geri al / yinele |
| Ctrl+Shift+C / Ctrl+Shift+V | Düzenlemeyi kopyala / yapıştır |
| Ctrl+R | Düzenlemeyi sıfırla |
| Ctrl+← / Ctrl+→ | Önceki / sonraki fotoğraf |
| Ctrl+0 / Ctrl+1 | Sığdır / %100 |
| 1–5, 0 | Yıldız puanı, puanı sil |
| P, X, U | Seç, reddet, işareti kaldır |
| C | Kırp |
| Y | Önce ve sonra |
| S | Ekran provası |
| L | Büyüteç |
| Shift+F | Odak vurgusu |
| Shift+C | Karşılaştır |
| Alt+1 … Alt+4 | A–D sürümleri |
| E | Dışa aktar penceresi |
| Tab | Panelleri gizle veya göster |
| Esc | Açık pencereyi kapat |

| Fare | İşlem |
| :--- | :--- |
| Tekerlek | İmleç konumunda yakınlaştır |
| Shift+tekerlek | Yana kaydır |
| Alt+tekerlek | Döndür |
| Sürükleme (sol veya orta düğme) | Kaydır |
| Fotoğrafa çift tık | Sığdır veya %200 |
| Bölüm başlığına Ctrl+tık | Yalnızca o bölümü açık bırak |
| Sürüm harfine sağ tık | Geçerli düzenlemeyi o sürüme kopyala |

## 16. Komut satırı, denetim soketi ve MCP

**Motor komutları.** Her komut tek bir JSON nesnesi yazdırır; böylece betikler ve ajanlar kullanabilir.

```bash
omastudio inspect DSCF0001.RAF                      # fotoğraf makinesi, objektif, pozlama, histogram, sahne
omastudio export DSCF0001.RAF --recipe '{"exposure":0.5}' --options '{"format":"jpeg"}'
omastudio fuji-recipe DSCF0001.RAF                  # fotoğraf makinesinin tarifi
omastudio fuji-pixel-shift DSCF*.RAF --out merged.dng
omastudio xmp export DSCF0001.RAF
omastudio cameras GFX                               # RAW çözücünün desteklediği fotoğraf makineleri
omastudio gpu                                       # demozaikleme için kullanılan GPU
omastudio npu                                       # ONNX Runtime ve hızlandırıcılar
omastudio cloud status
omastudio --cli help                                # tüm komutlar
```

**Denetim soketi.** `omastudio ipc <yöntem> [bağımsız değişkenler]` açık pencereyle konuşur; örneğin `openPhoto`, `openFolder`, `applyRecipe`, `applyPreset`, `setExposure`, `setWarmth`, `setMode essentials|pro`, `setRightTab`, `showSection`, `setView peaking|compare|softProof|split on|off`, `setZoom`, `showExport`, `showCloud`, `getStatus` ve `screenshot <dosya.png>`. Soket yalnızca aynı kullanıcıya yanıt verir.

**MCP.** `omastudio mcp` bir Model Context Protocol sunucusudur; YZ ajanları fotoğrafları inceleyebilir, işleyebilir, ayıklayabilir ve dışa aktarabilir, Fujifilm tariflerini okuyabilir ve açık pencereyi yönetebilir. Claude Code için: `claude mcp add omastudio -- omastudio mcp`. `omastudio skill` ajan becerisini `~/.claude/skills` klasörüne kurar.

## 17. Dosyalar ve klasörler

| Yol | İçerik |
| :--- | :--- |
| `<fotoğraf>.omastudio` | Bir fotoğrafın düzenlemesi (yan dosya) |
| `~/.local/share/omastudio/catalog.json` | Katalog: puanlar, işaretler ve renk etiketleri |
| `~/.local/share/omastudio/fuji-recipes/` | Kaydedilmiş Fujifilm tarifleri |
| `~/.local/share/omastudio/icc/` | Kendi ICC profilleriniz |
| `~/.local/share/omastudio/models/` | Yerel YZ modelleri |
| `~/.local/share/omastudio/film-luts/` | Fotoğraflarınızdan ölçülen film simülasyonu LUT'ları (`fuji-calibrate`) |
| `~/.config/omastudio/` | Ayarlar, arayüz durumu, dil, bulut hesapları |
| `~/.cache/omastudio/` | Küçük resimler, önizlemeler ve indirilen bulut fotoğrafları |

## 18. Sorun giderme

- **Pencere açılmıyor.** `omastudio` komutunu bir terminalde çalıştırın ve iletiyi okuyun. `omastudio --version` ve `omastudio status` nelerin kurulu olduğunu gösterir.
- **GPU kullanılmıyor.** `omastudio gpu` aygıtı gösterir. `vulkan-icd-loader` paketini ve ekran kartınızın Vulkan sürücüsünü kurun. `OMASTUDIO_GPU=off` CPU kullanımını zorlar.
- **USB'de fotoğraf makinesi bulunamıyor.** USB modunu USB RAW CONV./BACKUP RESTORE yapın, yeniden bağlayın ve paketin udev kuralının kurulu olduğunu denetleyin.
- **Bir bulut hizmeti bir program gerektiğini söylüyor.** Pencerenin adını verdiği paketi kurun, örneğin `sudo pacman -S rclone libsecret`.
- Hakkında penceresindeki **Sistem bilgisini kopyala** bir hata bildirimi için sürümü, GPU'yu ve Omarchy bilgilerini kopyalar. Sorunları [github.com/ozdil/omarchy-omastudio/issues](https://github.com/ozdil/omarchy-omastudio/issues) adresine bildirin.
