# PLA'yı kurmak ve kaldırmak

## Kurulum

1. GitHub Releases'tan `PLA_<sürüm>_x64-setup.exe` dosyasını indir. Yanındaki `.sha256` dosyası
   indirdiğin dosyanın bozulmadığını doğrulamak içindir:

   ```powershell
   Get-FileHash .\PLA_0.1.0_x64-setup.exe -Algorithm SHA256
   ```

2. Kurulumu çalıştır. Yönetici izni istemez; PLA yalnız senin kullanıcı hesabına,
   `%LOCALAPPDATA%\PLA` altına kurulur.
3. **"Windows bilgisayarınızı korudu" uyarısı:** PLA henüz kod imzası taşımıyor, bu yüzden
   SmartScreen bilinmeyen yayıncı uyarısı gösterir. **Ek bilgi → Yine de çalıştır** ile devam et.
4. Windows 10'da WebView2 yoksa kurulum onu kendiliğinden yükler (internet gerekir); Windows 11'de
   zaten vardır.

Dil modeli kuruluma dahil değildir: ilk açılışta PLA önerilen modeli gösterir ve onayınla indirir.

## Güncelleme

Yeni sürümün kurulumunu çalıştırman yeterli; notların, görevlerin, ayarların, indirilen modeller ve
"Windows ile başlat" seçimin olduğu gibi kalır. Kurulum eski sürümü kaldırırken kaldırıcının
penceresini gösterebilir: oradaki "Verilerimi de sil" kutusu güncellemede dikkate alınmaz.
PLA kendiliğinden güncellenmez ve güncelleme denetimi için internete bağlanmaz.

## Kaldırma

**Ayarlar → Uygulamalar → PLA → Kaldır.** Kaldırıcı bir kutu gösterir:

- **"Verilerimi de sil (görevler, ayarlar, modeller)" boş (varsayılan):** yalnız program silinir.
  Yeniden kurarsan her şey kaldığı yerden sürer.
- **İşaretli:** şunlar da silinir:
  - `%APPDATA%\PLA`: ayarlar, görevler, metrikler, asistan geçmişi;
  - `%LOCALAPPDATA%\PLA`: indirilen dil ve embedding modelleri (program da buradadır).

Kutu boşken modeller program silindikten sonra da `%LOCALAPPDATA%\PLA\models` içinde kalır.
Veritabanı yedekleri vault'un içindeki `.pla\backup` klasöründedir; kaldırma onlara dokunmaz.

Bu klasörlerden birinin içinde başka bir yere giden bir bağlantı (junction) ya da bir vault varsa
kaldırıcı o klasörü silmez ve sana hangi klasörü bıraktığını söyler.

**Notların (vault klasörün) hiçbir durumda silinmez.** PLA vault'u kendi veri klasörlerinin içine
koymana izin vermez; yine de orada bir vault bulunursa kaldırıcı o klasörü silmeden bırakır.

## Verilerin nerede

| Ne | Nerede |
| --- | --- |
| Notların | Seçtiğin vault klasörü (PLA'ya ait değil) |
| Ayarlar, görevler, metrikler, asistan geçmişi | `%APPDATA%\PLA` |
| Dil ve embedding modelleri | `%LOCALAPPDATA%\PLA\models` |
| Program | `%LOCALAPPDATA%\PLA` (modellerin yanında) |

## Kurulum dosyasını kendin derlemek

```bash
cd apps/desktop
npm run fetch-llama -- --from <llama.cpp b11280 Windows x64 CPU klasörü>
npm run release
```

Kurulum ve SHA-256 dosyası `dist-release/` klasörüne yazılır. `PLA_SIGN_COMMAND` ortam değişkeni
tanımlıysa (örneğin `signtool sign /fd sha256 /a %1`; `%1` ayrı bir argüman olmalı) kurulum ve
program onunla imzalanır. Tauri bunun için Windows SDK'daki `signtool`'u da arar.
