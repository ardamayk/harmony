# Bit-perfect çalma

Bu belge, Desktop Client'ta müziğin bit-perfect çalması için gerekenleri anlatır. Bit-perfect, dosyadaki örneklerin DAC'a hiç değişmeden ulaşması demektir. Bunun için yeniden örnekleme, ses kısma, karıştırma ya da efekt uygulanmamalıdır.

Buradaki sonuçlar 18 Ekim 2026'da Linux'ta ölçüldü. Ortam: PipeWire 1.6.9, gömülü mpv 0.41.0, Moondrop Dawn Pro2. Ölçüm yöntemi null test: çıktı yakalandı ve dosyadan çözülen referansla örnek örnek karşılaştırıldı. Ölçüm ayrıntıları `bit-perfect-rapor.md` dosyasında.

## Ses yolu

```text
FLAC dosyası → Music Server /stream → masaüstü medya proxy'si → mpv (çözme)
            → [Normal / Adaptive] PipeWire → ALSA → DAC
            → [Exclusive]        ALSA hw:X,Y  → DAC
```

| Aşama | Bit'leri değiştirir mi |
|---|---|
| Music Server ve medya proxy'si | Hayır. Dosya bayt bayt iletilir (SHA-256 aynı). |
| mpv çözme | Hayır, ancak mpv sesi %100 olmalı. |
| PipeWire | Graf hızı kaynak hızıyla aynıysa, bütün sesler %100'deyse ve başka bir kaynaktan duyulur ses gelmiyorsa hayır. Bu koşullardan biri sağlanmazsa evet. |
| ALSA `hw` (Exclusive) | Hayır. Dönüştürme eklentisi yoktur. |

Electron'un kendi ses yolu (tarayıcıdaki `<audio>`) bit-perfect değildir. Desktop Client sesi bu yüzden mpv ile çalar.

## Çıkış modları

### Normal

PipeWire grafı tek bir hızda çalışır. Çalışan bir akış varken bu hız değişmez. Bir tarayıcı sekmesi sessiz bile olsa açık bir akış tutar ve grafı (genellikle) 48 kHz'de sabitler. Ölçümde de 48 kHz dışındaki her parça yeniden örneklendi. Normal mod yalnızca kaynak hızı graf hızına eşitse bit-perfect olur.

### Adaptive

Uygulama, PipeWire'ın `clock.force-rate` ayarını parçanın hızına zorlar. DAC her parçada o hıza geçer (44,1 ile 192 kHz arasında ve 16/24 bitte ölçüldü, fark sıfır). Başka uygulamaların akışları PipeWire tarafından bu hıza dönüştürülür.

- Başka bir kaynak açık ama sessizse: bit-perfect.
- Başka bir kaynaktan gerçekten ses geliyorsa: bit-perfect değil. Karışım iki sesin toplamıdır, bu kaçınılmaz. Bu durumda Exclusive kullanılmalı ya da o kaynak başka bir cihaza yönlendirilmelidir.
- Moddan çıkınca, uygulama kapanınca ya da çöküş sonrası ilk açılışta `clock.force-rate` sıfırlanır.

### Exclusive

Uygulama, aktif PipeWire sink'inin kart profilini kapatır ve mpv'yi doğrudan ALSA `hw:X,Y` cihazına bağlar. Bu modda PipeWire devrede değildir ve diğer uygulamaların sesi başka bir cihaza geçer.

- DAC, parçanın formatında açılır: 16 bit için `S16_LE`, 24 bit için `S32_LE`, hız da parçanın hızı olur.
- Parçalar arasında cihaz açık kalır. Sonraki parça, mevcut parçanın bitmesine 10 saniye kala mpv'nin çalma listesine eklenir. Formatı değişen bir parçada cihaz yeni formatla yeniden açılır.
- Moddan çıkınca kart profili geri gelir ve PipeWire DAC'ın ses ayarını kendi değerine geri yükler.
- Ham ALSA eşlemesi olmayan çıkışlarda (Bluetooth gibi) Exclusive kullanılamaz.

### WASAPI (Windows)

Bu bölüm ölçülmedi, çıkarımdır. Shared modda Windows motoru tek bir hızda ve float olarak karıştırır. Doğru yol exclusive moddur (`ao=wasapi`, `--audio-exclusive=yes`). Parça geçişinde cihazın açık tutulması Windows'ta da aynı derecede önemlidir.

## Ses ayarları

Linux'ta dört ayrı ses ayarı vardır:

| Ayar | Nerede kısar | Bit-perfect'i bozar mı |
|---|---|---|
| Uygulamadaki müzik sesi (mpv) | Bilgisayarda, yazılımla | Evet, %100'ün altında |
| KDE / PipeWire'da uygulama akışının sesi | Bilgisayarda, yazılımla | Evet, %100'ün altında |
| KDE / PipeWire'da cihaz sesi | DAC'ın USB donanım ses kontrolünde | Çoğu seviyede hayır (aşağıya bakın) |
| DAC'ın kendi düğmesi | DAC'ın içinde | Hayır. Bunu DAC bilgisayara bildirmez, bu yüzden KDE'deki değer güncellenmez (çıkarım). |

En güvenli yöntem: uygulama sesini ve akış sesini %100'de tutmak, sesi DAC'ın düğmesinden ya da cihaz sesinden ayarlamak.

### Cihaz sesi nasıl çalışır

KDE kaydırıcısı değeri PipeWire'a iletir. Donanım ses kontrolü olan bir DAC'ta PipeWire bu değeri DAC'ın kontrolüne yazar. Dawn Pro2'de bu kontrol 0 ile -60 dB arasında, 0,5 dB'lik adımlarla çalışır. Ölçülen değerler:

| KDE | dB | DAC kontrolü | PipeWire yazılımsal kısma | Bit-perfect |
|---|---|---|---|---|
| %100 | 0 | 120 | 1,0 | evet |
| %50 | -18,1 | 84 | 1,0 | evet |
| %18 | -44,7 | 31 | 1,0 | evet |
| %15 | -49,4 | 22 | 0,951 | hayır |
| %10 | -60 | 0 (alt sınır) | 1,0 | evet |
| %8 | -65,8 | 0 | 0,51 | hayır |

- Kaydırıcı kübik çalışır: %18 zaten yaklaşık -45 dB'dir. %10'da DAC kendi alt sınırına iner. Bunun altındaki kısma tamamen yazılımla yapılır ve bit-perfect bozulur. Kulaklıkta bu seviyeler pratikte sessizdir.
- Değer bir donanım adımına tam oturmazsa PipeWire aradaki farkı yazılımla kapatır. Bunu kontrol etmek için: `pw-dump <sink-id>` çıktısında `softVolumes` değerinin `1.0` olması gerekir.
- Donanım ses kontrolü olmayan cihazlarda cihaz sesi her zaman yazılımsaldır.

## Ani yüksek sese karşı koruma

Exclusive ve Adaptive modlarında mpv %100'de çalışmak zorundadır. Bu moda geçerken mpv'deki kısma kaybolsaydı ses aniden yükselirdi (örneğin %40'tan %100'e çıkmak +8 dB demek). Uygulama bunu şöyle önler:

1. Moda geçerken önce cihazın sesini, mpv'deki kısma kadar kısar. Değer 0,5 dB adımlarına yuvarlanır. Exclusive'de bu kısma ALSA kartının ses kontrolünde, Adaptive'de PipeWire'ın varsayılan sink'inde yapılır.
2. Sonra mpv'yi %100'e çıkarır. Böylece duyulan ses değişmez, yalnızca kısmanın yapıldığı yer değişir.
3. Cihaz kısmanın tamamını karşılayamazsa (alt sınıra gelinmişse) değişiklik geri alınır ve mpv sesi olduğu gibi kalır. Ses yükselmez, ama o durumda çalma bit-perfect olmaz.
4. Moddan ya da uygulamadan çıkarken önce mpv sesi geri gelir, sonra cihaz sesi. Böylece geçiş sırasında ses yalnızca kısılabilir, hiçbir an yükselmez. Kaydedilen mpv sesi de kullanıcının gerçek seviyesi olarak kalır.

Bilinen sınırlar:

- Adaptive'de cihaz sesi kısıldığı için diğer uygulamaların sesi de aynı oranda kısılır.
- Bu modlarda uygulamadaki ses kaydırıcısı hâlâ mpv sesini değiştirir. Kaydırıcıyı %100'ün altına çekmek bit-perfect'i bozar.
- Adaptive açıkken uygulama çökerse sistem sesi kısık kalır. Bu güvenli yönde bir hatadır.

## Bit-perfect kontrol listesi

- Exclusive ya da Adaptive seçili olmalı.
- Uygulama sesi %100 olmalı, EQ ve ReplayGain kapalı olmalı (Direct profil).
- KDE / PipeWire'daki uygulama akışının sesi %100 olmalı.
- Adaptive'de başka bir kaynaktan ses gelmemeli.
- Cihaz sesi bir donanım adımında olmalı (`softVolumes` = 1.0) ya da %100'de tutulup ses DAC'ın düğmesinden ayarlanmalı.
- Kontrol: `/proc/asound/cardN/pcm0p/sub0/hw_params` dosyasındaki `rate` ve `format`, parçanın hızı ve bit derinliğiyle uyumlu olmalı.
