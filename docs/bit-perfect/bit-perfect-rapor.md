# Bit-perfect ses yolu: ölçüm raporu (Linux, 18 Ekim 2026)

**Ortam:** Moondrop Dawn Pro2 (card 3, `hw:3,0`), PipeWire 1.6.9, WirePlumber 0.5.18, gömülü mpv 0.41.0. Tüm ölçümlerde mpv sesi 1,0 (%100).
**Test sinyali:** Tam ölçekli beyaz gürültü (genlik 0,5). Bu sinyal 16 ve 24 bitin tüm bitlerini kullanıyor. FLAC olarak hazırlandı: 44,1 / 48 / 88,2 / 96 / 192 kHz × 16 / 24 bit. Ayrıca gerçek bir FLAC parçası (Nightwish, 24 bit / 44,1 kHz) kullanıldı.
**Yöntem:** Null test. Çıktı yakalandı, ffmpeg ile çözülen referansla örnek örnek karşılaştırıldı. "0 fark" ifadesi, karşılaştırılan penceredeki her örneğin bit düzeyinde aynı olduğu anlamına geliyor.
Gerçek DAC testleri kullanıcının izniyle yapıldı. Sonrasında `clock.force-rate=0`, kart profili `output:analog-stereo`, varsayılan sink Dawn Pro2 ve ses %18 olarak eski hâline geri alındı.

## 1. Sunucu → proxy → mpv

| Aşama | Sonuç | Kanıt |
|---|---|---|
| Sunucu `/stream` + masaüstü proxy'si | Bit-perfect | Akışın SHA-256 değeri = dosyanın SHA-256 değeri = `contentSha256`; Range isteği birebir aynı; hız 318 MB/s |
| mpv çözme (ses 1,0) | Bit-perfect | 2.645.998 örnekte 0 fark |
| mpv çözme (ses 0,68) | Bozuyor | Örneklerin %100'ü farklı |
| mpv → ALSA (ALSA `file` eklentisi, 16 ve 24 bit; 44,1 / 96 / 192 kHz) | 6/6 bayt bayt aynı | mpv 16 bit için `s16`, 24 bit için `s32` yazıyor; dönüştürme yok |

## 2. Gerçek DAC: Normal ve Adaptive

Yakalama gerçek sink'in monitöründen yapıldı (DAC'a giden karışım). DAC'ın gerçekte açılan formatı `/proc/asound/card3/pcm0p/sub0/hw_params` dosyasından okundu. Ortamda Zen'in açık ama sessiz akışları vardı; bu, gerçek kullanım durumu.

| Kaynak | Normal: DAC formatı | Normal: sonuç | Adaptive: DAC formatı | Adaptive: sonuç |
|---|---|---|---|---|
| 16 / 44,1 | S32_LE 48000 | Bozuk (yeniden örnekleme) | S32_LE 44100 | 0 fark |
| 16 / 48 | S32_LE 48000 | 0 fark | S32_LE 48000 | 0 fark |
| 16 / 88,2 | S32_LE 48000 | Bozuk | S32_LE 88200 | 0 fark |
| 16 / 96 | S32_LE 48000 | Bozuk | S32_LE 96000 | 0 fark |
| 16 / 192 | S32_LE 48000 | Bozuk | S32_LE 192000 | 0 fark |
| 24 / 44,1 | S32_LE 48000 | Bozuk | S32_LE 44100 | 0 fark |
| 24 / 48 | S32_LE 48000 | 0 fark | S32_LE 48000 | 0 fark |
| 24 / 88,2 | S32_LE 48000 | Bozuk | S32_LE 88200 | 0 fark |
| 24 / 96 | S32_LE 48000 | Bozuk | S32_LE 96000 | 0 fark |
| 24 / 192 | S32_LE 48000 | Bozuk | S32_LE 192000 | 0 fark |

Başka bir kaynağın olduğu durumlar (24 bit; 44,1 ve 96 kHz):

| Durum | Sonuç |
|---|---|
| Adaptive + sessiz başka kaynak (48 kHz) | 0 fark (iki hızda da) |
| Adaptive + sesli başka kaynak (440 Hz ton) | Bozuk: karıştırma yapılıyor, bu kaçınılmaz |
| Normal + sesli başka kaynak | Bozuk |
| Adaptive, force-rate mpv başladıktan sonra ayarlandı (uygulamadaki sıra), başka kaynak önceden açık | 0 fark (ayrı test örneğinde) |

**Normal neden bozuluyor:** Zen'in açık akışları PipeWire grafını 48 kHz'de tutuyor. PipeWire çalışan grafın hızını değiştirmediği için 48 kHz dışındaki her kaynak yeniden örnekleniyor. Hiçbir akış yokken graf ilk akışın hızına geçiyor ve Normal de 0 fark veriyor (ayrı test örneğinde ölçüldü).
**Adaptive:** DAC her kaynak hızına geçiyor. Sessiz kaynaklar bit'leri değiştirmiyor. Başka bir kaynaktan gerçekten ses geliyorsa ortak bir sink'te bit-perfect olmak mümkün değil; o kaynağı başka bir cihaza yönlendirmek ya da Exclusive kullanmak gerekiyor.

## 3. Gerçek DAC: Exclusive (ALSA `hw:3,0`, kart profili kapalı)

| Kaynak | mpv çözme → çıkış | DAC hw_params | Çalma ortasında xrun |
|---|---|---|---|
| 16 / 44,1 … 192 (5 hız) | s16 → s16, hız aynı | S16_LE, kaynak hızı | 0 |
| 24 / 44,1 … 192 (5 hız) | s32 → s32, hız aynı | S32_LE, kaynak hızı | 0 |
| Gerçek FLAC 24 / 44,1, 90 saniye | | | 0 |

Raw `hw` yolunda dönüştürme eklentisi yok ve mpv'nin yazdığı veri bayt bayt aynı (bölüm 1). Bu yüzden Exclusive bit-perfect. DAC'ın çıkışını dijital olarak geri kaydedemediğim için bu sonuç, `hw_params` ve ALSA eklentisiyle yapılan yakalamanın birleşiminden çıkıyor.

**Parça geçişi (kesintinin nedeni):**

| Geçiş şekli | Cihaz kaç kez açıldı | Geçişte XRUN | Ne oluyor |
|---|---|---|---|
| Uygulamadaki şekil (EOF sonrası `loadfile replace`) | 2 | 1 | 1. parça tampon boşalana kadar çalıyor, cihaz kapanıyor ve yeniden açılıyor; mpv tarafında yaklaşık 100 ms boşluk var, USB DAC'ın yeniden kilitlenmesi buna ekleniyor |
| Çalma listesine önceden ekleme (`loadfile append`) | 1 | 0 | Cihaz açık kalıyor, gapless geçiş |

Ortada xrun hiç yok (16 çekirdek tam yükteyken de 0). Exclusive'deki kesinti, uygulamanın her parça sonunda DAC'ı kapatıp açmasından kaynaklanıyor.

## 4. Ses seviyesi

KDE kaydırıcısı DAC'ın USB donanım ses kontrolüne yazıyor (`PCM Playback Volume`: 0 ile -60 dB arası, 0,5 dB adım).

| KDE | Hedef dB | DAC kontrolü | PipeWire `softVolumes` | Bit-perfect |
|---|---|---|---|---|
| %100 | 0 | 120 | 1,0 | evet |
| %50 | -18,1 | 84 | 1,0 | evet |
| %26 | -35,1 | 50 | 1,0 | evet |
| %18 | -44,7 | 31 | 1,0 | evet |
| %15 | -49,4 | 22 | 0,951 | **hayır** |
| %12 | -55,3 | 10 | 1,0 | evet |
| %10 | -60 | 0 | 1,0 | evet |
| %8 | -65,8 | 0 | 0,51 | **hayır** |
| %2 | -102 | 0 | 0,008 | **hayır** |

- Kaydırıcı kübik çalışıyor: %18 zaten -44,7 dB. %10'da DAC kendi alt sınırına (-60 dB) iniyor ve bunun altı tamamen yazılımla kısılıyor. %18'in altında sesin neredeyse hiç duyulmamasının nedeni bu.
- Donanım adımına tam oturmayan bazı değerlerde (ölçülen: %15) PipeWire aradaki farkı yazılımla kapatıyor ve bit'ler değişiyor.
- Uygulama akışının sesi (pavucontrol veya KDE'de uygulama bazlı ses) her zaman yazılımsal.
- DAC'ın düğmeleri DAC'ın kendi iç kademesini değiştiriyor gibi görünüyor (çıkarım). DAC bunu bilgisayara bildirmediği için KDE güncellenmiyor. Bu kademe USB verisinden sonra çalıştığı için bit'leri değiştirmez.

## 5. Uygulamadaki sorunlar

1. **Hatırlanan ses, Exclusive ve Adaptive'i Processed'e geçiriyor.** `restore_remembered_volume` (`desktop/core/src/lib.rs:479`) `processing-settings.json` içindeki `alsa:hw:3,0 = 0.4` ve `system = 0.68` değerlerini geri yüklüyor. Bu değerler profili Processed'e çeviriyor ve mpv sesi 1'in altına düşüyor. Mod seçimi tek başına bit-perfect garanti etmiyor.
2. **Parça geçişi DAC'ı kapatıp açıyor.** Sonraki parça EOF'tan sonra `loadfile replace` ile yükleniyor (`playback.rs:469`, `:895`). Bu da Exclusive'de kesintiye neden oluyor (bölüm 3).
3. Doğrudan ALSA hatasında çalma duraklıyor ve sessizce Normal'e dönülüyor (`playback.rs:3134`). Doğrulanmadı ama "kesinti" gibi görünebilir.

## 6. Öneriler (kod değişikliği yapılmadı)

1. Exclusive ve Adaptive'de hatırlanan sesi geri yüklemeyin ve Direct profilini koruyun. Ses seviyesini DAC'ın donanım kontrolüne ya da düğmelerine bırakın.
2. Sonraki parçayı EOF'tan önce mpv'nin çalma listesine ekleyin (`loadfile <url> append`, `--prefetch-playlist=yes`). Ölçüldü: cihaz açık kalıyor, geçişte xrun yok. Hız değişirse `gapless-audio=weak` cihazı yine doğru hızla yeniden açar.
3. Adaptive'de "sesli başka kaynak" için tek çözüm, o kaynağı başka bir sink'e yönlendirmek ya da Exclusive kullanmak.

## 7. WASAPI (yalnızca akıl yürütme)

Shared modda Windows motoru tek bir hızda ve float olarak karıştırır. Bu yüzden ancak motor hızı kaynak hızıyla aynıysa, ses %100'deyse ve APO/efekt yoksa bit-perfect olabilir. Doğru yol exclusive mod (`ao=wasapi`, `--audio-exclusive=yes`). Parça geçişinde cihazı açık tutmak (öneri 2) Windows'ta da aynı derecede önemli.

## Kapsanmayanlar

- DAC'ın analog çıkışı ve iç DSP'si (dijital geri kayıt imkânı yok).
- Adaptive'de farklı hızdaki iki parça arasındaki geçiş gerçek DAC'ta ölçülmedi.
- DAC düğmelerinin davranışı doğrulanmadı.
