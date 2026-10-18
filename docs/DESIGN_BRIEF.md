# Earthly Audio — Ürün & Tasarım Brief

**Marka:** Earthly Audio — *The Modern Craftsman*  
**Repo:** Navidrome Replacement (self-hosted music server)

Bu doküman, projenin ne olduğunu, nereye gittiğini ve nasıl bir arayüz hedeflendiğini anlatır.

### Güncel UI (Earthly shell)

- **3 kolon:** sol nav + widget dock | orta içerik | sağ queue (sabit üst) + widget dock
- **Panel resize:** mouse ile handle sürükle; `layout.sizes` server’da sync
- **Widget DnD:** now-playing, discover sol/sağ arası taşınabilir; queue sabit
- **Tema:** `earthly` (teal/toprak/yeşil) ve `tokyo-night` preset + light/dark/system
- **Nav:** Favorites, Albums, Folders, Radio, Tracks, Playlists, Settings (API’siz olanlar placeholder)

---

## 1. Proje nedir?

**Navidrome Replacement**, kendi sunucunda çalışan, kendi müzik kütüphaneni stream eden bir **self-hosted müzik sunucusu**dur. Navidrome’dan ilham alır ama onun bir kopyası değildir:

- Eski **Subsonic API** kullanılmaz; modern **OpenAPI** tabanlı özel API
- **Modüler monolith** backend (Go) + **widget tabanlı** özelleştirilebilir arayüz
- İleride **plugin / sidecar** ile genişletilebilir mimari (keşif, indirme, party mode vb.)
- Tek binary ile NAS / ev sunucusunda hafif çalışma hedefi

Kısa tanım (elevator pitch):

> *Kendi müziğini barındırdığın, arayüzünü panel ve widget’larla düzenleyebildiğin, modern ve genişletilebilir bir self-hosted müzik sunucusu.*

---

## 2. Neden yapıyoruz?

| Navidrome’da eksik / yetersiz | Bu projede hedef |
|------------------------------|------------------|
| Party listen (birlikte dinleme) | Core + ileride WebSocket |
| Playlist paylaşımı | Planlı |
| Yeni müzik keşfi | Plugin / sidecar (opt-in) |
| İndirme / offline | Opt-in sidecar (legal ayrım) |
| Preview | Planlı |
| Plugin ekosistemi yok | Modüler monolith + resmi sidecar’lar |
| Subsonic API’ye bağlı | Özel API; ileride isteğe bağlı uyumluluk |

**Temel ilkeler:**

1. **Self-host first** — kullanıcı verisi kendi makinesinde
2. **Legal risk ayrımı** — gri alan özellikler opt-in, ayrı repo / sidecar
3. **Özelleştirme** — layout ve widget’lar kullanıcı tercihine göre (server’da sync)
4. **Tek kod tabanı** — web + ileride native client aynı `@repo/ui` ve API’yi kullanır

---

## 3. Kim kullanacak?

| Persona | İhtiyaç |
|---------|---------|
| **Self-hoster** | NAS / mini PC’de müzik kütüphanesi, tarayıcıdan dinleme |
| **Power user** | Panel yerleşimi, tema, queue, ileride plugin |
| **Aile / arkadaş** | Party listen, paylaşılan playlist (gelecek) |
| **Geliştirici** | OpenAPI, modüler backend, sidecar entegrasyonu |

Platform hedefi: **önce web (responsive + PWA)**, sonra opsiyonel desktop (Tauri) ve mobil client.

---

## 4. Teknik bağlam (tasarımcı için özet)

Tasarımı kodla hizalamak için:

| Katman | Ne |
|--------|-----|
| **Backend** | Go, SQLite, yerel klasörden library scan |
| **API** | REST `/api/v1/...`, OpenAPI spec |
| **Web** | React SPA, shadcn + Tailwind |
| **Paylaşılan UI** | `packages/ui` — AppShell, widget’lar, player bar |

Production’da tek Go binary web arayüzünü de serve eder. Geliştirmede API `:8090`, web dev `:3000`.

**Tasarım kısıtları:**

- Desktop-first müzik deneyimi; mobilde player bar + library responsive
- Dark / light / system tema (tercih server’da saklanır)
- Kapak görseli v1’de çoğunlukla **harf avatar placeholder** (gerçek art sonra)
- Subsonic client’larına benzemek zorunlu değil — kendi kimliğimiz

---

## 5. Arayüz felsefesi: Modüler widget shell

Arayüz sabit tek sayfa değil; **üç bölge + alt player** mantığı:

```
┌──────────┬────────────────────────────┬──────────┐
│  Sol     │         Ana içerik         │  Sağ     │
│  panel   │    (Library, Settings…)    │  panel   │
│          │                            │          │
│  Nav +   │                            │ Widget’lar│
│ Widget’lar│                           │ (opsiyonel)│
├──────────┴────────────────────────────┴──────────┤
│   ╭── Player bar (floating, orta kolon içinde) ──╮  │
└───────────────────────────────────────────────────┘
```

### Özelleştirilebilir layout (server sync)

Kullanıcı tercihleri API’de tutulur; tüm cihazlarda aynı layout:

- **sidebarPosition:** `left` | `right` — hangi yan panel “birincil”
- **panels.left / panels.right:** widget id listesi (sıra önemli)
- **collapsed:** sol/sağ panel daraltılmış mı

Varsayılan widget’lar:

| Widget id | Açıklama | Durum |
|-----------|----------|--------|
| `now-playing` | Şu an çalan parça, mini progress | ✅ Çalışıyor |
| `queue` | Sıradaki parçalar, tıkla çal, sil | ✅ Çalışıyor |
| `discover` | Yeni müzik önerileri | 🔜 Placeholder |

İleride eklenebilecek widget’lar (tasarımda slot olarak düşün): lyrics, recently played, party guests, scrobble status.

**Settings** sayfasında: tema + layout kontrolleri (sidebar tarafı, panel aç/kapa). İleride **Customization** sekmesi genişleyecek (webhook, keşif kaynakları toggle vb.) — şimdilik Settings yeterli.

---

## 6. Ekran envanteri

### 6.1 Library (ana sayfa) — `/library`

**Amaç:** Kütüphanedeki albümleri keşfetmek.

**İçerik:**

- Sayfa başlığı: “Library”
- **Scan banner** — kütüphane taraması durumu + “Scan library” aksiyonu (admin)
- **Album grid** — responsive kart grid
  - Her kart: kapak (placeholder harf), albüm adı, sanatçı
  - Tıklanınca → Album detail

**Boş durum:** Henüz scan yok / albüm yok → yönlendirici mesaj + scan CTA

**Yükleme / hata:** Skeleton veya kısa mesaj

---

### 6.2 Album detail — `/library/:albumId`

**Amaç:** Bir albümün parça listesi; dinlemeye başlama.

**İçerik:**

- Albüm başlığı, sanatçı, (ileride) yıl / kapak büyük
- **Track list** — satır: track no, başlık, süre, play aksiyonu
- Satıra tıklayınca queue’ya eklenir / çalmaya başlar

---

### 6.3 Settings — `/settings`

**Amaç:** Görünüm ve layout kişiselleştirme.

**Bölümler:**

1. **Theme** — Light / Dark / System (segmented veya toggle group)
2. **Layout** — Sidebar left/right, sol/sağ panel collapse
3. (Opsiyonel) Server bilgisi — API sürümü, health (şu an küçük footer metni)

İleride buraya veya ayrı **Customization** sayfasına: webhook, keşif kaynakları, party ayarları.

---

### 6.4 Global: Sidebar navigation

Sol panelin üst kısmında (widget’ların üstünde):

- Uygulama adı / logo alanı
- **Library** (aktif state)
- **Settings**
- İleride: Playlists, Discover, Party (nav item olarak)

Aktif route vurgusu net olmalı.

---

### 6.5 Global: Player bar (floating alt)

Her sayfada görünür; Spotify / Apple Music benzeri ama kopya değil. Yan panellerin arasında, orta kolonun altında yüzen (floating) yuvarlatılmış bir kart: sağdan/soldan ve alttan 16px içeride durur, içerik altında kaymaya devam eder.

**Sol:** Küçük kapak placeholder + track title + artist (truncate)

**Orta:** Play/Pause, Next, seek slider, current time / duration

**Sağ:** Quality pill (tıklayınca çıkış modu menüsü), volume ikonu (hover/focus ile yukarı açılan dikey slider, tıklama mute), Lyrics (alttan yukarı açılan tam ekran görünüm: solda sözler, sağda kuyruk), en sağda queue toggle

**Durumlar:**

- Nothing playing — disabled kontroller, placeholder metin
- Playing — progress hareket eder
- Queue boşken next disabled

Mobil: orta blok sadeleşebilir; seek hâlâ erişilebilir.

---

### 6.6 Yan panel widget’ları

**Now Playing (widget):**

- Track + artist
- İnce progress bar
- Mini play/pause

**Queue (widget):**

- Scrollable liste
- Aktif parça vurgulu
- Satır: başlık, sanatçı, remove (×)
- Tıkla → o parçadan çal

**Discover (widget) — gelecek:**

- “Because you listened to…”
- Yeni çıkanlar (kütüphanedeki sanatçılardan)
- Last.fm / ListenBrainz benzeri entegrasyon (sidecar)

---

## 7. Gelecek ekranlar (tasarım sistemine dahil et, implement sonra)

Bunları Figma’da **wireframe / “Coming soon”** olarak planlamak iyi olur:

| Ekran / özellik | Kısa açıklama |
|-----------------|---------------|
| **Playlists** | Oluştur, düzenle, paylaşım linki, collaborative |
| **Party mode** | Ortak queue, host, oy verme, canlı sync göstergesi |
| **Discover (full page)** | Keşif feed, filtreler, kaynak seçimi |
| **Login** | Çok kullanıcı gelince; şimdilik tek stub user |
| **Admin / Sources** | Music path, scan zamanlaması, sidecar enable |
| **Public share link** | Playlist veya album read-only görünüm |
| **Mobile PWA** | Bottom nav veya drawer; player bar compact |

---

## 8. Görsel yön önerileri

Mevcut kodda **shadcn neutral** + hafif **yeşil/teal** accent (`styles.css` içinde lagoon/sand tonları). Tasarımda:

- **Hissiyat:** Sakin, odak müzikte; göz yormayan dark mode önemli
- **Yoğunluk:** Desktop’ta bilgi zengin; gereksiz chrome yok
- **Kapaklar:** Grid’de dominant görsel; placeholder da tutarlı (rounded, harf veya gradient)
- **Widget’lar:** Kart benzeri, border + hafif arka plan; panel içinde stack
- **İkonlar:** Lucide tarzı ince çizgi (play, skip, volume, library, settings)

**Referans his (kopyalama değil):** Navidrome’un sade yapısı + Spotify’ın player bar netliği + Jellyfin’in self-host hissi.

**Kaçınılacaklar:**

- Aşırı neon / gamer estetiği
- Subsonic client’larıyla birebir aynı layout
- Çok fazla üst menü — nav sidebar + player bar yeterli

---

## 9. Şu an implement edilenler (v1)

Tasarımı mevcut koda map ederken:

| Özellik | Durum |
|---------|--------|
| Library scan (yerel klasör) | ✅ |
| Album grid + album detail + track list | ✅ |
| Playback queue + HTTP Range stream | ✅ |
| Player bar | ✅ |
| Widget: now-playing, queue | ✅ |
| Widget: discover | Placeholder |
| Settings: theme + layout | ✅ |
| Playlists, party, auth | ❌ |

---

## 10. Figma / Stitch için pratik notlar

1. **Frame boyutları:** Desktop 1440×900 ana; Player bar yüksekliği ~72–80px; yan panel ~256px açık / ~40px collapsed
2. **Component set:** Button, Card, TrackRow, AlbumCard, PlayerBar, WidgetCard, SidebarNav, ScanBanner
3. **Variants:** Theme light/dark; panel collapsed; empty / loading / error states
4. **Auto-layout:** Album grid responsive (4→3→2 kolon); widget stack vertical gap 12px
5. **İsimlendirme:** Frame’ler İngilizce (`Library/AlbumGrid`, `Global/PlayerBar`) — kod route’larıyla uyumlu
6. Bu brief + canlı dev server (`mise run dev` → `http://localhost:3000/library`) birlikte kullanılabilir

---

## 11. İlgili dosyalar

| Dosya | İçerik |
|-------|--------|
| [README.md](../README.md) | Kurulum, stack |
| [AGENTS.md](../AGENTS.md) | Geliştirici / agent kuralları |
| [contracts.md](../contracts.md) | API sözleşmesi |
| [packages/docs/content/layout-customization.mdx](../packages/docs/content/layout-customization.mdx) | Layout modeli (teknik) |

---

*Son güncelleme: Modular Monolith v1 (Library + Playback) tamamlandıktan sonra.*
