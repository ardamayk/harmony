# Sonradan bakılacaklar

Şimdilik ertelenen ama unutulmaması gereken işler. Bir madde bitince buradan silinir.

## Şarkı sözü ekranı: kapak, playback ve kuyruk tek sütunda

- **Fikir:** Kapak, küçük bir playback ve altında kuyruk tek bir sütunda dursun; sözler yanda büyük gösterilsin. Bugünkü üç sütunlu düzende sözler sıkışıyor.
- **Prototip:** [`packages/ui/prototypes/lyrics-side-column.prototype.html`](../packages/ui/prototypes/lyrics-side-column.prototype.html). Tarayıcıda açılır, varyantlar `?variant=A|B|C` ya da ← → ile değişir. **Bu dosya silinmeyecek.**
- **Beğenilenler:** A ve B.
  - **A:** kapak, altında küçük playback, onun altında kuyruk; sözler sağda.
  - **B:** küçük kapak başlığın yanında, kuyruk sütunun çoğunu kaplıyor; sözlere en fazla yer kalıyor.
  - C (sözler solda, sütun sağda) seçilmedi.
- **Ortak fikirler:** Sözler Spotify'daki gibi akıyor (aktif satır parlak, geçmiş satırlar soluk, sıradakiler orta tonda, yumuşak geçiş). Üst barda küçültme düğmesinin yanında pencere ve sayfa ikonları var.
- **Uygulamadan önce bakılacaklar:** dar pencerede kuyruğun gizlenmesi, açık tema, Masaüstü İstemcisi ve şu anki `LyricsOverlay.tsx` ile hangi parçaların birleştirileceği.
