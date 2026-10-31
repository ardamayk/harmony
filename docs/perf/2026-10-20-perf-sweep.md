# Performans taraması, 2026-10-20

Bu tarama, beş Sonnet 5.5 ajanıyla yapılan lab ölçümlerine dayanıyor; RUM verisi yok. Ölçümler tek bir `flock` kilidiyle sırayla yapıldı.

**Kurulum ve sınırlar.** `data/app.db` boş olduğu için sentetik kütüphaneler kullanıldı: 30 track, 500, 4k, 5k, 20k ve 60k track. Server olarak 17 Ekim'de derlenmiş, HEAD'den eski `bin/server` kullanıldı. Web, headless Chromium'da throttle olmadan ölçüldü. Desktop playback ölçülmedi, çünkü ortamda mpv ve ses cihazı yok. Ölçüm sırasında kullanıcının kendi `desktop:dev` süreci de çalışıyordu, bu yüzden wall-clock rakamlarda gürültü olabilir. Sayaç ölçümleri (commit, DOM, istek sayısı) bundan etkilenmez.

Ham çıktılar ve tekrar üretme script'leri bu oturumun scratchpad dizininde: `perf/a1…a5`. Klasörlerin karşılığı şöyle:
- a1: playback re-render
- a2: büyük listeler ve navigasyon
- a3: string ve regex
- a4: layout shift ve persist
- a5: server ve desktop

## Etkiye göre sıralı bulgular

### 1. Çalarken bütün ağaç saniyede ~4 kez yeniden çiziliyor (a1, a2)
- **Sebep:** `packages/ui/src/playback/PlaybackProvider.tsx:632`. `currentTime` tek context değerinin içinde ve `useMemo` `session`'a bağlı. Bu yüzden `usePlayback()` çağıran 20 bileşenin hepsi her `timeupdate`'te yeniden render oluyor. Bu değeri gerçekten kullanan yalnızca yaklaşık 7 bileşen var.
- **Ölçüm (a1, 30 track, dev build):**
  - /library/tracks sayfasında saniyede 3.7 commit, commit başına ~716 render. Toplam ~2,670 render/sn.
  - Çalmıyorken 0 commit.
  - Arama kutusuna yazarken, çalma sürüyorsa render sayısı 7 kat artıyor (1,384 yerine 9,920).
- **Ölçüm (a2, prod build):** 5,000 satırlık bir playlist açıkken 6 saniyede 15 long task ve 6.1 s bloklama oluştu. `timeupdate` olayları saniyede 23'ten 14-15'e düşüyor.
- **Düzeltme:** `currentTime`, `duration` ve `bufferedEnd` ayrı bir context'e alınmalı (`usePlaybackTime()`). Bu context'i yalnızca şunlar kullanır:
  - `PlayerBar` (seek bar ve saat)
  - `LyricsOverlay`
  - `widgets/registry.tsx:14`

  Ardından `memo(TrackRow)` ve `memo(QueueTrackRow)` eklenmeli. Ayrıca `use-sleep-timer.ts:73` içindeki `mode` nesnesi `useMemo` ile sarılmalı.

### 2. Playlist sayfası tüm satırları çiziyor ve kapakları eager yüklüyor (a2)
- **Yer:** `-playlist-page.tsx:115` ve `track-list.tsx:237`. Cover `<img>`'de `loading="lazy"` yok.
- **Ölçüm, 5,000 satır:**
  - 65k DOM node
  - ilk render 2.0 s
  - 1.3 s long task
  - 302 MB heap
  - 20k kütüphanede 1,894 görsel isteği
- **Düzeltme:** önce `loading="lazy"` eklenmeli. Büyük playlist'ler için satırlar ayrıca virtualize edilmeli.

### 3. Server liste sorguları pahalı (a5, 60k track)
- **Liste endpoint'leri:** albums, tracks ve artists'te p50 128–153 ms, yaklaşık 60 req/s ve istek başına ~100 ms CPU. Detay, cover ve waveform endpoint'leri ise 1 ms'in altında.
- **Sanatçıya göre track filtresi:** c=1'de 406 ms, c=8'de yaklaşık 1 s (`library/repository.rs:335,354`). Satır başına iki korelasyonlu `EXISTS` ile tam tarama yapılıyor ve sanatçı index'leri kullanılmıyor.
- **Albums ve artists:** `LIMIT`'ten önce bütün kütüphane aggregate ediliyor (66 ve 37 ms/sayfa).
- **Tracks count:** filtre olmasa bile bütün `library_track_reads` view'ı taranıyor (~40 ms). Offset 50000'de 145 ms.
- **Fuzzy arama:** tokio worker thread'inde çalışıyor (`search/service.rs:78`). Sonuç bulamayan bir aramada 130 ms CPU harcıyor ve bu arada worker'ı bloke ediyor. `spawn_blocking`'e taşınmalı.

### 4. Gereksiz ağ trafiği ve refetch (a2, a4)
- **Playlists index:** her kart için playlist'in tamamı çekiliyor (`-playlists-page.tsx:75`). 20k kütüphanede bu 6.2 MB demek ve aynı veri IndexedDB'ye de yazılıyor.
- **Favorites:** app root'ta ve neredeyse her navigasyonda tamamı çekiliyor (`use-favorite-tracks.ts:20-27`). 1,000 favoride bu 0.9 MB. Her satır için `includes()` çağrılıyor, `Set` kullanılmıyor.
- **Varsayılan `staleTime` 0** (`main.tsx:23`). Geri gelindiğinde 195 KB yeniden çekiliyor, 3,235 node baştan kuruluyor ve 70-92 ms long task oluşuyor. Ayrıca global `refetchOnWindowFocus: true` açık.
- **Hover prefetch yok:** `defaultPreload: "intent"` yalnızca route chunk'larını önceden yüklüyor, veriyi değil.
- **Albums sayfası** aynı 500 albümü iki kez çekiyor (`-albums-page.tsx:42-60`).
- **Root layout** `/preferences` isteği bitene kadar `Loading…` gösteriyor (`__root.tsx:142`). Oysa `defaultPreferences` zaten var.
- **/radio** açılışta her istasyon için ayrı bir now-playing isteği atıyor.

### 5. Embedded server SPA'yı sıkıştırmadan ve cache'siz sunuyor (a2)
- **Yer:** `server/src/static_assets.rs` yalnızca `content-type` header'ını set ediyor.
- **Etki:** her yüklemede ~1.0 MB JS gidiyor. Gzip ile bu 334 KB olurdu.
- **Düzeltme:** `CompressionLayer` eklenmeli, `/assets/*` için de `Cache-Control: immutable` verilmeli.

### 6. Desktop açılışı sıralı yapılıyor (a5)
- **Sebep:** pencere, core'un `shell:ready` olayını bekledikten sonra oluşturuluyor (`desktop/src/main/main.ts:104-108`).
- **Etki:** ready-to-show ~370 ms. Bu sürenin yaklaşık %28'i bekleme.
- **Düzeltme:** pencere `whenReady` anında oluşturulursa tahminen ~265 ms'e iner. Bu bir tahmin, denenmedi.
- **Compile cache:** etkisiz çıktı, gerek yok.

### 7. Sıralamada `localeCompare` options ile çağrılıyor (a3)
- **Yer:** `album-filters.tsx:35,88`, `collect-genres.ts:26`, `sort-albums.ts:8` ve radio sayfası.
- **Etki:** paylaşılan bir `Intl.Collator`'dan ~60x yavaş. 10k öğede 203 ms'e karşı 3.3 ms.
- **Kapsam:** yalnızca sanatçı, albüm, tür ve istasyon listelerinde, veri yüklenince bir kez çalışıyor.
- **Türkçe karakter etkisi küçük:** UTF-16 olan string'ler yalnızca ~1.4x yavaş. Bu fark sadece `filterTracksByText`'te görünüyor, o da 20k track'te ~8.5 ms sürüyor ve bir frame'e sığıyor.

### 8. Küçük layout shift'ler ve üçüncü taraf font (a4)
- **Shift'ler:** en kötü sayfa /radio, 0.0117. Bütçe 0.1 olduğu için Lighthouse bunu yakalamıyor.
- **Muhtemel sebep:** `AppShell.tsx:100` içindeki `transition-padding`. Kuyruk paneli padding'i ilk boyamadan sonra netleşiyor ve değişim animasyonla oluyor. Bu sebep tek başına izole edilmedi.
- **Font:** `web/src/styles.css:1` fontları Google Fonts'tan `@import` ediyor. Bu /radio'daki kaymanın ~0.004'ü. Daha önemlisi self-hosted ve üçüncü taraf istek yok ilkesine aykırı. WOFF2 dosyaları self-host edilmeli, preload ve fallback metrikleri eklenmeli.
- **Persist:** bu boyutta sorun değil. 1.1 MB cache'te bir yazım 1–3 ms sürüyor. Kodda `location.reload()` yok.

## Doğruluk riskleri (performans dışı)
- **Cache key çakışması:** `more-from-artist.tsx:15` ile `-artist-page.tsx:56` aynı query key'i kullanıyor ama farklı miktarda veri çekiyor. Sonuç olarak artist sayfası bazen yalnızca 100 albüm gösterebilir. Bu kod okumasıyla bulundu, ölçülmedi.
- **Türkçe "İ" araması:** `filterTracksByText` düz `toLowerCase()` kullanıyor. Bu yüzden "istanbul" araması "İstanbul"u bulamayabilir. Test edilmedi.
- **Liste sınırları:** albums, artists ve tracks sayfaları 500, 100 ve 200 öğede kesiliyor. 20k kütüphanede track'lerin %99'u ancak arama ile görülebiliyor.

## Açıklanamayanlar
- `perf:server-load` her koşuda 7128 MB disk yazımı raporluyor. Sıralı denemede bu tekrar üretilemedi.
- `bin/server` SIGTERM'de kapanmadı, ajanlar SIGKILL kullanmak zorunda kaldı.
- Yük sonrası RSS 385 MB. Neyin tuttuğu incelenmedi.

## Ratchet önerisi
- **Server ve desktop:** seed'lenmiş bir kütüphane ve commit'lenmiş bir `perf-baseline.json` kullanılır. Median ölçüm tavanı %15'ten fazla aşarsa CI kırılır. Tavanlar yalnızca aşağı iner (a5).
- **Layout shift:** Lighthouse'un toplam CLS'inin yanına, bölge başına ~0.002 eşikli bir Playwright kontrolü eklenir (a4).
- **Re-render:** "çalarken commit başına render" sayacı. Deterministik olduğu için ratchet'e en uygun adayı bu (a1).

## Önerilen sıra
1. Playback'i iki context'e ayırmak. En büyük etki bu, değişiklik de küçük.
2. Row cover'lara `loading="lazy"` eklemek ve varsayılan `staleTime`'ı 30–60 s yapmak.
3. Server'da compression ve asset cache header'ları.
4. Artist filtresi sorgusu, albums/artists aggregate'leri ve fuzzy aramayı `spawn_blocking`'e taşımak.
5. Playlists index ve Favorites için hafif endpoint'ler.
6. Desktop penceresini paralel açmak.
7. Fontları self-host etmek.
