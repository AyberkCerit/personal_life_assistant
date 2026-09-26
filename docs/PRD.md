# Ürün Gereksinimleri Dokümanı (PRD)

| | |
|---|---|
| **Proje** | PLA — Edge AI Destekli Yerel Kişisel Asistan *(çalışma adı)* |
| **Yazar** | Ayberk Cerit |
| **Sürüm / Durum** | v0.3 — Taslak |
| **Tarih** | 2026-09-26 |
| **Platform (MVP)** | Masaüstü, Windows öncelikli (Tauri sayesinde macOS/Linux sonradan) |

### Sürüm geçmişi

**v0.3 (gözden geçirme sonrası)**
- Veritabanı ikiye bölündü: `pla.db` (kullanıcı verisi, vault'a yedeklenir) ve `cache.db` (yeniden üretilebilir) (§3, §5.1).
- Blok kimliği hash yerine bulanık eşleştirmeyle belirleniyor; "kullanıcı kazanır" kuralı ve reddedilen öğelerin hatırlanması eklendi (§5.2).
- Göreli tarihlerin referansı "bugün" değil, notun tarihi ya da bloğun yazıldığı an (§5.2).
- Otomatik üretilen içerik çıkarımı tetiklemiyor; günlük özetler vault'tan çıkarılıp `cache.db`'ye taşındı (§5.2, §6).
- Embedding modeli Rust süreci içinde çalışıyor; soru-cevap için iki modelin birlikte RAM bütçesi tanımlandı (§3, §4).
- Çıkarım, yazarken her 2 saniyede değil; not kapanınca ya da boşta toplu olarak tetikleniyor (§5.2).
- Yeni mimari ilke: hesabı Rust/SQL yapar, model yalnız dil üretir. Bağlam penceresi 8k ve bütçe paylaşımı tanımlandı (§3, §6).
- Gece 02:00 bakımı yerine "günde bir kez, ilk boşta kalma anında" kuralı; uykudan uyanma algılanıyor (§5.6).
- Metrik türleri birleştirme kurallarıyla tanımlandı (§5.4).
- Görevler ve hatırlatıcılar için ayrı bölüm; bildirim mekanizması (§5.3).
- Ağ izolasyonu Rust katmanında ve CSP ile tanımlandı (§9).
- Değerlendirme setleri büyütüldü ve ayrıldı; araç çağırma M1'de ölçülüyor (§7).
- Kilometre taşları yeniden sıralandı: i18n altyapısı M0'da, editör inceltildi, yapay zekâ işçisi öne alındı (§13).

**v0.2**
- Kapsam: yalnız masaüstü, bireysel proje, çoklu dil hedefi.
- Yapay zekâ iki modda: **arka plan işçisi** + **soru-cevap / komut paneli**.
- Ayrı bir C++ katmanı yok; llama.cpp hazır bir süreç (`llama-server`) olarak Rust'tan yönetiliyor.
- Ham veri **asla silinmez**; özetler yalnız ek bir katman.
- SQLite-vss / Chroma yerine `sqlite-vec`.
- Terim düzeltmesi: "Gizli Enjeksiyon / Prompt Injection" → **RAG ile bağlam ekleme** (prompt injection bir saldırı türüdür, bkz. §9).
- Sağlık/verimlilik metrikleri: notlardan çıkarım + hızlı giriş paneli.
- Tarih hesabı modelden alınıp Rust'a verildi.
- Model uyarlama stratejisi: fine-tuning MVP sonrası ve koşullu (§8.2).
- Obsidian uyumluluğu bir tasarım ilkesi (§5.1).

---

## 1. Ürün Vizyonu ve Kapsamı

**Amaç:** Kullanıcının notlarını, görevlerini ve sağlık/verimlilik verilerini tamamen çevrimdışı tutan; yapay zekâyı arka planda çalışan otonom bir veri işleyici ve istendiğinde soru yanıtlayıp komut uygulayan yerel bir asistan olarak konumlandıran kişisel üretkenlik uygulaması.

**Etkileşim modları**

| Mod | Ne yapar | Tetikleyici |
|---|---|---|
| Arka plan işçisi | Notlardan görev/hatırlatıcı/metrik çıkarır, günlük bakımı ve periyodik raporları üretir | Not kapanışı / boşta kalma, zamanlayıcı |
| Soru-cevap / komut | Notlar ve veriler üzerinden soruları yanıtlar, görev ekleme gibi işlemleri uygular | Kullanıcı |

### Kapsam içi (MVP)
- %100 yerel çalışma; ağ yalnız model indirmek için kullanılır (§8.1, §9).
- Uygulamanın kendi vault'u: Obsidian uyumlu, Markdown tabanlı, bağlantı ve etiket destekli not sistemi (§5.1).
- Otomatik görev / hatırlatıcı / metrik çıkarımı (§5.2).
- Görev paneli ve hatırlatıcı bildirimleri (§5.3).
- Sağlık ve verimlilik metrikleri (§5.4).
- Soru-cevap ve komut paneli (§5.5).
- Zamanlanmış işler, sistem tepsisi, kaçırılan işlerin telafisi (§5.6).
- Arayüzde en az Türkçe ve İngilizce; notlar herhangi bir dilde yazılabilir (§7).

### Kapsam dışı (MVP)
- Mobil platformlar.
- Bulut senkronizasyonu, çok cihaz, çok kullanıcı.
- Uygulama içi disk şifrelemesi (V2'de opsiyonel ayar).
- Obsidian ile aynı anda düzenlemede canlı eşitleme (dosya izleyici). MVP'de dış değişiklik algılanır ve üzerine yazılmaz (§5.1).
- Notlardaki `- [ ]` onay kutularının görevlerle iki yönlü eşitlenmesi (V2).
- Tekrarlayan görevler (V2).
- Giyilebilir cihaz entegrasyonu, sesli komut.
- Bulut LLM API'leri.
- Modelin fine-tuning ile uyarlanması (MVP sonrası, §8.2).

---

## 2. Hedef Kullanıcı

Birincil kullanıcı geliştiricinin kendisi; ürün kişisel kullanım üzerinden olgunlaştırılır. Genel hedef kitle: verisini buluta vermek istemeyen, not tutan ve rutinlerini takip eden bilgi çalışanları ve öğrenciler (özellikle Obsidian kullanıcıları).

---

## 3. Teknoloji Yığını ve Sistem Mimarisi

| Katman | Teknoloji | Görev |
|---|---|---|
| Sunum | Tauri 2 + Svelte 5 + TypeScript (Vite), Markdown editörü CodeMirror 6 | Koyu tema odaklı arayüz, editör, görev/metrik panelleri, soru-cevap paneli, sistem tepsisi, bildirimler |
| İş mantığı | Rust | Vault ve dosya işlemleri, veritabanları, zamanlayıcı, çıkarım hattı, hesaplamalar, araç (tool) yürütme, model sürecinin yaşam döngüsü |
| LLM çıkarımı | llama.cpp (`llama-server` alt süreci) | GGUF dil modelini CPU/GPU üzerinde çalıştırmak; gramer/JSON şemasıyla kısıtlı çıktı |
| Embedding | Rust süreci içinde (ör. `fastembed-rs` / ONNX Runtime) | Not parçalarını ve soruları vektöre çevirmek |
| Depolama | Vault (`.md`) + `pla.db` + `cache.db` (`sqlite-vec` ile) | Aşağıdaki "asıl kaynak" kuralına göre |

**Mimari ilkeler**
- **C++ kodu yazılmaz.** llama.cpp hazır ikili olarak paketlenir ve Rust tarafından sidecar süreç olarak başlatılıp durdurulur. Süreci sonlandırmak modelin RAM'ini kesin olarak serbest bırakır ve "yükle → işle → boşalt" stratejisine birebir uyar. Alternatif: `llama-cpp-2` crate'i ile süreç içi bağlama (açık karar, §12).
- `llama-server` yalnız `127.0.0.1` üzerinde, rastgele portta ve her çalıştırmada üretilen bir API anahtarıyla dinler. Onunla yalnız Rust konuşur; arayüz doğrudan erişemez.
- **Asıl kaynak kuralı:**

  | Depo | İçerik | Yeniden üretilebilir mi? | Nerede |
  |---|---|---|---|
  | Vault (`.md`) | Notlar, haftalık raporlar | Hayır (kullanıcı içeriği) | `Belgeler/PLA Vault` |
  | `pla.db` | Görevler, metrikler, reddedilen öğe kayıtları, blok eşleştirme kayıtları, iş kayıtları, ayarlar | **Hayır** | `%APPDATA%/PLA`; günlük yedeği vault'ta `.pla/backup/` |
  | `cache.db` | Not indeksi (FTS5), vektörler (`sqlite-vec`), günlük özetler | Evet (silinirse baştan oluşturulur) | `%APPDATA%/PLA` |

  Not içeriğinin kopyası hiçbir veritabanında asıl kaynak olarak tutulmaz.
- **Hesabı Rust yapar, model yalnız dil üretir.** Tarih aritmetiği, toplam, ortalama, trend ve karşılaştırmalar SQL/Rust'ta hesaplanır. Model bu değerleri yalnız yorumlar ve anlatır. Küçük modeller aritmetikte güvenilmezdir.

```
[Svelte UI] ⇄ (Tauri IPC) ⇄ [Rust çekirdek] ──► vault/*.md
                                  │  ├── pla.db   (kullanıcı verisi)
                                  │  ├── cache.db (indeks, vektörler, özetler)
                                  │  ├── embedding (süreç içi)
                                  │  └── zamanlayıcı / iş kuyruğu
                                  ▼
                        [llama-server alt süreci]  (isteğe bağlı başlatılır, işi bitince kapatılır)
```

---

## 4. Donanım ve Performans

Uygulama **isteğe bağlı yükleme** (on-demand load) stratejisiyle çalışır: dil modeli sürekli açık kalmaz.

**Minimum hedef donanım (MVP)**
- x86-64 CPU, 4 çekirdek, AVX2
- 8 GB RAM
- ~5 GB boş disk (model + uygulama + veri)
- GPU opsiyonel (Vulkan/CUDA hızlandırması bir açık karar, §12)

| Metrik | Hedef | Not |
|---|---|---|
| Boşta RAM | ≤ 150 MB | Tauri + Rust + WebView2 süreçlerinin toplamı; ölçümle doğrulanacak. |
| Aktif çıkarım RAM | LLM ≤ 2 GB; embedding modeli ≤ 300 MB | Soru-cevap sırasında ikisi birlikte yüklüdür: toplam ≤ 2.3 GB. |
| LLM soğuk yükleme | ≤ 5 sn (SSD) | |
| Çıkarım (bir not, toplu) | ≤ 10 sn (yalnız CPU) | Arka planda; kullanıcıyı bloklamaz. |
| Soru-cevap ilk token | ≤ 5 sn soğuk, ≤ 2 sn sıcak | Soru embedding'i süreç içinde, yüklemesiz. |
| LLM tutma süresi | Son istekten 60 sn sonra kapatılır | Soru-cevap oturumunda her soruda yeniden yükleme olmasın diye. |

---

## 5. Temel Özellikler

### 5.1. Vault ve Markdown Not Sistemi
Obsidian benzeri, ancak uygulamanın kendi içinde inşa edilen bir not sistemi.

**Tasarım ilkesi: Obsidian uyumluluğu.** PLA vault'u Obsidian'da açılabilir kalmalı, bir Obsidian vault'u da PLA'da açılabilmelidir. Böylece kullanıcı Obsidian'ı bırakmadan PLA'yı arka plan işçisi olarak kullanabilir.
- Yalnız Obsidian'ın anladığı sözdizimi kullanılır: CommonMark + GFM, `[[not]]`, `[[not|takma ad]]`, `[[not#başlık]]`, `![[gömme]]`, `#etiket`, `#iç/içe` etiketler, YAML frontmatter (`tags`, `aliases`).
- PLA'ya özgü veri notların gövdesine yazılmaz. Gerekirse yalnız frontmatter'da `pla_` önekli alanlar kullanılır (ör. `pla_generated: true`); vault düzeyindeki ayarlar `.pla/` klasöründedir.
- `.obsidian/` klasörüne dokunulmaz. Dosya adlarında Obsidian'ın desteklemediği karakterler (`[ ] # ^ | \ :` vb.) kullanılmaz.
- Sistem klasörlerinin yolları (`daily/`, `reports/`…) `.pla/config` üzerinden değiştirilebilir. Böylece mevcut bir vault'un kendi düzeni (ör. Obsidian'ın günlük not klasörü) kullanılabilir.
- **Dış değişiklik güvenliği:** Kaydetmeden önce dosyanın değiştirilme zamanı ve hash'i kontrol edilir. Dosya dışarıda (ör. Obsidian'da) değişmişse üzerine yazılmaz, kullanıcıya çakışma gösterilir. Uygulama açılışında ve pencere odağa geldiğinde değişen dosyalar yeniden indekslenir.

**Konum ve düzen**
- Vault varsayılan olarak `Belgeler/PLA Vault` altındadır (ilk açılışta değiştirilebilir); notlar düz `.md` dosyalarıdır.
- Veritabanları ve model dosyaları vault'un **dışında**, `%APPDATA%/PLA` altında durur; böylece vault bulut senkron klasörüne konsa bile açık SQLite dosyaları bozulmaz.
- **Yedek:** `pla.db`'nin tutarlı bir kopyası günlük bakımda (§5.6) SQLite yedekleme işleviyle `.pla/backup/pla-YYYY-MM-DD.db` olarak vault'a yazılır; son 7 kopya tutulur. Böylece **vault klasörünü kopyalamak tam yedek demektir.**
- **Klasör düzeni:** sabit sistem klasörleri + serbest kullanıcı alanı. Diskteki adlar sabit ve İngilizcedir; arayüz bunları seçili dilde gösterir (ör. `daily/` → "Günlük"), dil değişince klasörler yeniden adlandırılmaz.

  ```
  Belgeler/PLA Vault/
  ├── .pla/
  │   ├── config                 vault ayarları, sistem klasörü yolları
  │   └── backup/                pla.db günlük yedekleri (son 7)
  ├── inbox/                     hızlı yakalama
  ├── daily/2026/2026-09-26.md   günlük notlar
  ├── notes/                     serbest alan (alt klasörler kullanıcının)
  ├── reports/weekly/2026-W39.md haftalık raporlar (otomatik, pla_generated)
  ├── attachments/
  └── templates/

  %APPDATA%/PLA/
  ├── pla.db                     kullanıcı verisi (görevler, metrikler, kayıtlar)
  ├── cache.db                   indeks, vektörler, günlük özetler (yeniden üretilebilir)
  ├── models/                    *.gguf, embedding modeli
  └── logs/
  ```

**Editör ve not özellikleri**
- YAML frontmatter (ör. `tags`, `aliases`, `created`, `updated`).
- `[[wikilink]]` bağlantıları, geri bağlantı (backlinks) paneli, `#etiket` desteği.
- Klasör ağacı, hızlı dosya açma, tam metin arama (FTS5, `cache.db`).
- Notlardaki `- [ ]` onay kutuları MVP'de düz metindir; görevlerle iki yönlü eşitleme V2'dedir.
- Otomatik üretilen notlar (haftalık raporlar) frontmatter'da `pla_generated: true` taşır.

### 5.2. Otonom Veri Çıkarımı (Structured Extraction)
Kullanıcının serbest metin olarak yazdığı notlardan görev, hatırlatıcı ve metrik çıkarılır ve **otomatik olarak** eklenir.

**Örnek** (`daily/2026/2026-09-26.md` notunda)
- Metin: *"Yarın sabah 8'de kalkıp omuz çalışacağım, shoulder press hedefi."*
- Model çıktısı (göreli zaman, mutlak tarih yok):
  ```json
  {"items": [{"type": "task", "title": "Omuz antrenmanı", "details": "shoulder press",
              "when": {"day_offset": 1, "time": "08:00"}}]}
  ```
- Rust'ın kaydettiği öğe (referans tarih = notun tarihi 2026-09-26; mutlak tarih ve kaynak eklenmiş):
  ```json
  {"type": "task", "title": "Omuz antrenmanı", "details": "shoulder press", "date": "2026-09-27", "time": "08:00",
   "source": {"note": "daily/2026/2026-09-26.md", "block_id": "blk_8f3a"}, "origin": "extracted"}
  ```
- Sonuç: Görev, panele "AI tarafından eklendi" rozetiyle düşer.

**Kapsam**
- Çıkarım yalnız **kullanıcının yazdığı notlarda** çalışır. `reports/`, `templates/` klasörleri ve frontmatter'ında `pla_generated: true` olan notlar hariç tutulur. Böylece yapay zekânın ürettiği içerik yeni görev üretmez (döngü oluşmaz).

**Tetikleme**
- Yazarken tetiklenmez. Not kapatıldığında, başka nota geçildiğinde veya 45 sn boyunca yazılmadığında, o nottaki **değişen bloklar toplu olarak** işlenir.
- Aynı dakika içinde birden fazla not değişirse tek bir model yüklemesiyle sırayla işlenir.

**Zaman ve tarih**
- **Tarih hesabı modelde değil, Rust'ta.** Model zamanı yalnız göreli ve yapılandırılmış biçimde verir: `{"day_offset": 1}`, `{"day_offset": -1}` (geçmiş, metrikler için), `{"weekday": "friday", "which": "next"}`, `{"date": "2026-10-03"}` (yalnız metinde açık tarih varsa).
- **Referans tarih "bugün" değildir:** Günlük notlarda notun tarihi, diğer notlarda bloğun ilk yazıldığı an (`pla.db`'de tutulur) referans alınır. Böylece dünkü nota bugün yazılan "yarın" ya da ertesi gün telafi olarak işlenen bir blok doğru güne düşer. Modele bağlam olarak bu referans tarih ve haftanın günü verilir.

**Çıktı güvenceleri**
- **Kısıtlı çıktı:** JSON şeması / GBNF grameri kullanılır; sözdizimsel olarak geçersiz JSON üretilemez. Rust'ın anlamsal doğrulamasını (zorunlu alanlar, geçerli saat/tarih, metrik birimleri) geçemeyen çıktı eklenmez, **İnceleme** kutusuna düşer (§5.3).
- **Tür ayrımı:** Model gelecekteki niyetleri (`task` / `reminder`) geçmiş kayıtlardan (`metric`, örn. "dün 7 saat uyudum") ayırır.
- **Gerekirse iki adım:** Doğruluk yetersiz kalırsa çıkarım ikiye bölünür: önce sınıflandırma (görev / hatırlatıcı / metrik / hiçbiri), sonra yalnız ilgili türün alanları.

**Blok kimliği ve tekrar önleme**
- Her bloğa `pla.db`'de kalıcı bir `block_id` atanır; nota hiçbir işaret yazılmaz.
- Not yeniden işlendiğinde bloklar **bulanık eşleştirmeyle** eski kimliklerine bağlanır: nottaki konum + metin benzerliği (eşik ayarlanabilir). Hash yalnız "blok değişti mi?" kontrolü için kullanılır. Böylece bir yazım düzeltmesi yinelenen görev üretmez.
- Değişen bir blokta çıkarılan öğe **güncellenir**, yenisi eklenmez. Eşleşmeyen (silinmiş) bir bloğun öğesi "kaynağı silindi" olarak işaretlenir, silinmez.
- **Kullanıcı kazanır:** Kullanıcının panelde düzenlediği, tamamladığı veya sildiği bir öğeyi yapay zekâ bir daha değiştirmez ya da yeniden oluşturmaz.
- **Reddedilenler hatırlanır:** "Geri al" veya silme ile reddedilen öğeler blok bazında `pla.db`'ye kaydedilir; aynı blok yeniden işlendiğinde tekrar eklenmez.

**Diğer kurallar**
- **Geri alma:** Her otomatik ekleme bir bildirim ve "Geri al" eylemiyle gösterilir; öğeden kaynak nota tek tıkla gidilebilir.
- **Sınırlama:** Otomatik çıkarım yalnız **ekler/günceller**; hiçbir şeyi silmez.

### 5.3. Görevler ve Hatırlatıcılar
- **Görev paneli:** Elle ekleme, düzenleme, tamamlama ve silme; bugün / yaklaşan / tamamlanan görünümleri; kaynağa göre filtre (`manual`, `extracted`, `assistant`).
- **Görev alanları:** başlık, ayrıntı, tarih, saat, `notify_at`, durum (`open` / `done` / `cancelled`), köken (`origin`), kaynak (not + `block_id`), kullanıcı tarafından değiştirildi mi.
- **İnceleme kutusu:** Doğrulamayı geçemeyen çıkarımlar görev panelinde ayrı bir sekmede listelenir; kullanıcı düzeltip onaylar veya reddeder.
- **Hatırlatıcı** = `notify_at` alanı dolu bir görev. Zamanı gelince Tauri bildirim eklentisiyle Windows bildirimi gösterilir.
- Bildirimler uygulama tepsideyken çalışır (§5.6). Uygulama kapalıyken kaçırılan hatırlatıcılar açılışta "Kaçırılan hatırlatıcılar" listesinde gösterilir.
- Tekrarlayan görevler V2'dedir.

### 5.4. Sağlık ve Verimlilik Metrikleri
- **İki giriş yolu:** (a) notlardan otomatik çıkarım (§5.2), (b) küçük bir hızlı giriş paneli.
- Her kayıt; tarihini, kaynağını (`manual` / `extracted`) ve varsa kaynak notu taşır.
- **Birleştirme kuralı:** Her metrik türünün bir birimi ve bir birleştirme kuralı vardır. Böylece aynı değer hem panele hem nota yazıldığında iki kez sayılmaz.

  | Metrik (taslak) | Birim | Kural | Aynı gün iki kayıt varsa |
  |---|---|---|---|
  | Uyku | saat | Günde tek değer | Elle giriş önceliklidir; iki çıkarım çelişirse çakışma gösterilir |
  | Kilo | kg | Günde tek değer | Aynı |
  | Adım | adım | Günlük toplam (tek değer) | Aynı |
  | Su | ml | Toplanır | Kayıtlar toplanır; aynı bloktan gelen yinelenen kayıt sayılmaz |
  | Antrenman | egzersiz, set, tekrar, ağırlık | Oturum kaydı (çoklu) | Her oturum ayrı kayıttır |

- Basit görünüm: günlük/haftalık liste ve temel trend grafikleri. Tüm istatistikleri Rust/SQL hesaplar.

### 5.5. Soru-Cevap ve Komut Paneli
Sohbet uygulamanın merkezi değildir; bir yan panel veya komut paleti olarak açılır.
- **Okuma:** Notlar (RAG, §6), görevler ve metrikler (yapılandırılmış sorgular) üzerinden yanıt verir. Örn. *"Geçen hafta ortalama kaç saat uyudum?"*, *"Proje X hakkında ne not almıştım?"*
- **Sayısal yanıtlar hesaplanmış değerlerden gelir:** Ortalama, toplam, sayım gibi değerleri araçlar (`query_metrics`, `query_tasks`) SQL ile hesaplayıp döndürür; model yalnız bunları aktarır.
- **İşlem:** Araç çağırma (tool calling) ile komut uygular. Araç seti (taslak): `search_notes`, `query_tasks`, `query_metrics`, `add_task`, `complete_task`, `log_metric`, `create_note`.
- **Kaynak gösterme:** Yanıtlar kullandığı **ham notlara** bağlantı verir (özetlere değil). Yeterli bilgi yoksa "bulamadım" der, uydurmaz.
- **Onay kuralı:** Ekleme ve güncelleme doğrudan uygulanır ve geri alınabilir. **Silme** ve toplu değişiklikler açık onay ister.
- **Dil:** Yanıt, sorunun dilinde verilir.

### 5.6. Zamanlanmış İşler ve Arka Plan Çalışması
- **Günlük bakım:** Sabit bir saatte değil, **günde bir kez, ilk uygun boşta kalma anında** çalışır (varsayılan koşul: ≥ 5 dk kullanıcı hareketsizliği ve cihaz prizde). İçeriği:
  - Önceki günlerin günlük özetlerinin üretilmesi (`cache.db`)
  - Embedding indeksinin güncellenmesi
  - `pla.db`'nin vault'a yedeklenmesi (§5.1)
- **Haftalık rapor** (varsayılan Pazar 20:00): Girdisi ham notların tamamı değil; o haftanın **günlük özetleri + Rust'ın hesapladığı istatistikler** (tamamlanan görev sayısı, metrik ortalamaları ve trendleri). Çıktı, "Haftalık Verimlilik ve Strateji Özeti" olarak `reports/weekly/` altına `pla_generated: true` ile yazılır.
- **Tepsi modu:** Pencere kapatılınca uygulama sistem tepsisinde çalışmaya devam eder; bildirimler ve zamanlayıcı buradan yürür. Windows açılışında otomatik başlatma ayarlardan açılıp kapatılabilir.
- **Telafi:** Her işin son çalışma zamanı `pla.db`'de tutulur. Uygulama açılışında **ve bilgisayar uykudan uyandığında** kaçırılan işler kontrol edilir; her iş türü için yalnız en son kaçırılan örnek, boşta kalma koşulu sağlandığında çalıştırılır.
- **Nezaket:** Kullanıcı aktifken ağır işler ertelenir. Pil ve tam ekran algılama V2'dedir.

---

## 6. Hiyerarşik Hafıza Yönetimi

Amaç: Dil modelinin bağlam sınırını aşmadan ve RAM taşmasına (OOM) yol açmadan geçmiş bilgiye erişmek.

| Katman | İçerik | Saklama | Silinir mi? |
|---|---|---|---|
| 1. Ham veri | Notlar, görevler, metrikler | Vault `.md` + `pla.db` | **Hayır.** Yalnız kullanıcı siler. |
| 2. Özetler | Günlük yoğunlaştırılmış özetler (kaynak bağlantılarıyla) | `cache.db` (iç hafıza) | Yeniden üretilebilir |
| 3. Semantik indeks | Not parçalarının (chunk) embedding'leri | `cache.db` (`sqlite-vec`) | Yeniden üretilebilir |

- **Sıkıştırma** ham veriyi silmez; üst katmana özet ekler.
- **Hata birikimini önleme:** Özetler yapay zekâ çıktısıdır ve hata içerebilir. Bu yüzden:
  - Soru-cevapta özetler yalnız hangi güne/nota bakılacağını bulmak için kullanılır; bağlama öncelikle **ham not parçaları** girer.
  - Kaynak bağlantıları her zaman ham notları gösterir.
  - Haftalık raporlar RAG'de ham notlardan sonra gelir ve yeni özetlerin girdisi olmaz.
- **Bağlam oluşturma (soru-cevap):** 8k token'lık bağlam penceresi, sabit bir bütçe paylaşımıyla doldurulur. Bu işleme **RAG ile bağlam ekleme** denir.

  | Pay | İçerik |
  |---|---|
  | ~%40 | Son 48 saatin ham verisi (sığmazsa en yeniden eskiye doğru kırpılır) |
  | ~%40 | En ilgili *k* parça (vektör + FTS hibrit arama) |
  | ~%20 | Araçların döndürdüğü hesaplanmış değerler (görevler, metrik istatistikleri) |

- Embedding için çok dilli küçük bir model Rust süreci içinde çalışır (ör. *multilingual-e5-small* sınıfı, ONNX).

---

## 7. Çoklu Dil

- **Arayüz:** i18n altyapısı M0'da kurulur; MVP sonunda Türkçe ve İngilizce tamdır.
- **İçerik:** Notlar herhangi bir dilde olabilir. LLM ve embedding modeli çok dilli seçilir.
- **Değerlendirme setleri** (TR ve EN ayrı ayrı raporlanır):

  | Set | Boyut (en az) | Ölçtüğü |
  |---|---|---|
  | Çıkarım | Dil başına 100 (toplam 200) | Rust doğrulamasını geçme oranı; tür, başlık, göreli zaman, metrik değeri doğruluğu |
  | Soru-cevap | 30 soru | Doğruluk, kaynak gösterme, bilgi yokken "bulamadım" deme |
  | Araç çağırma | 30 senaryo | Doğru aracın doğru parametrelerle seçilmesi |

- **Model seçimi (M1):** Aday modeller (1.5–4B aralığı, Q4 kuantizasyon) arayüz olmadan basit bir test düzeneğiyle üç sette birden ölçülür; hız ve RAM de kaydedilir. Araç çağırma arayüzü M4'te gelse de model M1'de bu yetenek açısından da seçilir.
- Tarih ve sayı biçimleri arayüz diline göre gösterilir; veri her zaman ISO biçiminde saklanır.

---

## 8. Model Dağıtımı ve Uyarlama

### 8.1. Dağıtım
- Kurulum paketi dil modelini içermez. **İlk açılışta** kullanıcıya önerilen model gösterilir ve açık onayıyla indirilir. Embedding modeli küçük olduğu için kurulumla gelebilir (açık karar).
- İndirme sonrasında SHA-256 doğrulaması yapılır; indirme yarıda kalırsa devam ettirilebilir.
- İleri kullanıcı, diskteki kendi GGUF dosyasını seçebilir.
- İndirme tamamlandıktan sonra uygulama ağ erişimi olmadan tam işlevseldir.

### 8.2. Model Uyarlama Stratejisi
İlke: JSON'un **biçimini** gramer garanti eder; fine-tuning biçim için değil, küçük modelin **anlamsal doğruluğunu** (tür ayrımı, alanlar, Türkçe ifadeler) artırmak ve gerekirse daha küçük bir modelle yetinmek için yapılır. Fine-tuning **MVP kapsamı dışındadır.**

| Aşama | Ne yapılır | Ne zaman |
|---|---|---|
| 1. Taban çizgisi | Kısıtlı gramer + sistem talimatı + 3–5 örnek (few-shot) + tarih hesabının Rust'ta yapılması; 2–3 aday model değerlendirme setlerinde ölçülür (§7) | MVP (M1–M2) |
| 2. Karar noktası | §10'daki çıkarım hedefleri tutuyorsa fine-tuning yapılmaz. Tutmuyorsa önce daha büyük model (RAM bütçesi içinde) ve iki adımlı çıkarım denenir. | MVP sonu |
| 3. Fine-tuning | LoRA/QLoRA ile yalnız çıkarım görevine özel uyarlama | MVP sonrası (M7) |

**Aşama 3 ayrıntıları (MVP sonrası)**
- **Veri:** 1–3 bin etiketli TR + EN örnek. Büyük bir modelle sentetik not + doğru JSON üretimi (distillation), bir kısmının elle kontrolü ve geliştiricinin kendi notlarından örnekler. Değerlendirme seti eğitim verisinden tamamen ayrı tutulur.
- **Eğitim:** Python Ar-Ge ortamında (uygulamayla dağıtılmaz), Unsloth veya Hugging Face TRL ile QLoRA. Referans donanım: RTX 4050 Laptop (6 GB VRAM), 4B'ye kadar modeller.
- **Dağıtım biçimi:** (a) adaptör ana modelle birleştirilip kuantize edilir, tek GGUF dosyası olarak dağıtılır; ya da (b) adaptör GGUF'a çevrilir ve `llama-server`'a `--lora` ile yalnız çıkarım isteklerinde takılır. Seçim, soru-cevap ve özet kalitesinde gerileme olup olmadığına göre yapılır.
- **Ön koşul:** Çıkarım JSON şeması donmuş olmalıdır; şema değişikliği yeniden eğitim gerektirir.
- **Başarı ölçütü:** Aynı değerlendirme setinde taban çizgisine göre alan doğruluğunda ölçülebilir artış, ya da daha küçük bir modelle aynı doğruluk (daha az RAM, daha hızlı yanıt). Soru-cevap ve araç çağırma setlerinde gerileme olmamalı.
- **Bakım maliyeti:** Ana model değiştirildiğinde adaptör taşınamaz; eğitim yeniden yapılır.

---

## 9. Güvenlik ve Gizlilik

- **Sıfır telemetri:** Hiçbir kullanıcı verisi, alışkanlığı veya notu cihaz dışına çıkmaz. Otomatik güncelleme kontrolü MVP'de yoktur.
- **Ağ izolasyonu (katman katman):**
  - **Rust:** HTTP istemcisi yalnız model indirme modülünde bulunur ve sabit bir izinli adres listesiyle çalışır. Başka hiçbir modül ağa çıkamaz.
  - **Arayüz (webview):** Sıkı bir içerik güvenlik politikası (CSP) uygulanır; dış kaynak yüklenemez. Tauri yetki (capability) listesi yalnız gereken komutlara izin verir.
  - **Model sunucusu:** `llama-server` yalnız loopback adresinde, rastgele portta ve API anahtarıyla çalışır; iş bitince kapatılır. Arayüz ona doğrudan erişemez.
  - LLM çıkarımı, embedding ve vektör arama %100 yereldir.
- **Prompt injection'a karşı:** Notlar veya içe aktarılan metinler modele talimat gibi görünen içerik taşıyabilir. Bu nedenle:
  - Otomatik çıkarım hiçbir şeyi silemez ve yapay zekânın ürettiği notlarda çalışmaz.
  - Soru-cevaptaki silme ve toplu işlemler açık onay ister.
  - Modelin erişebildiği araçlar §5.5'teki sabit listeyle sınırlıdır; dosya sistemine veya ağa serbest erişimi yoktur.
- **Veri sahipliği:** Vault klasörü `pla.db` yedeklerini de içerdiği için vault'u kopyalamak tam yedektir. Görevler ve metrikler için CSV/JSON dışa aktarma sağlanır.
- **Şifreleme:** MVP'de dosyalar açık formattadır. Disk şifrelemesi işletim sistemine bırakılır (BitLocker önerilir). Uygulama içi şifreleme V2'de opsiyonel ayar olarak gelir.

---

## 10. Başarı Kriterleri (MVP kabul ölçütleri)

1. **Çıkarım kalitesi:** Çıkarım setinde sözdizimsel JSON geçerliliği %100 (gramer), Rust doğrulamasını geçme ≥ %95; tür, göreli zaman ve metrik alanlarında doğruluk ≥ %85 (TR ve EN ayrı ayrı).
2. **Tekrar önleme:** Bir bloğa yapılan yazım düzeltmesi yinelenen öğe üretmez; kullanıcının düzenlediği veya reddettiği öğe yeniden oluşmaz (otomatik testlerle).
3. **Kaynak:** Boşta ve çıkarım/soru-cevap sırasında §4'teki RAM hedefleri ölçümle sağlanır.
4. **Veri güvenliği:** Hiçbir otomatik işlem ham veri silmez. Yalnız vault klasörünün kopyasından, görevler ve metrikler dahil, tam geri yükleme test edilir.
5. **Güvenilirlik:** Kapalı geçen bir Pazar'dan sonra haftalık rapor telafi edilerek üretilir; kaçırılan hatırlatıcılar açılışta listelenir.
6. **Soru-cevap:** Soru-cevap setinde yanıtlar ham notlara kaynak gösterir, bilgi yokken uydurmaz; sayısal yanıtlar SQL sonuçlarıyla birebir tutar.
7. **Kullanım:** Geliştirici uygulamayı 4 hafta boyunca günlük asıl not/görev aracı olarak kullanır.

---

## 11. Riskler

| Risk | Etki | Önlem |
|---|---|---|
| Küçük modelin Türkçe ve analiz kalitesi düşük | Yanlış çıkarım, yüzeysel rapor | Değerlendirme setleriyle model seçimi; kısıtlı gramer; hesap ve tarih Rust'ta; model değiştirilebilir mimari; MVP sonrası fine-tuning (§8.2) |
| Küçük modelde araç çağırma güvenilir değil | Soru-cevap komutları hatalı | M1'de araç çağırmanın ölçülmesi; gramerle kısıtlı araç çağrısı, sınırlı araç seti, geri alma |
| Otomatik eklemenin gürültü üretmesi | Görev panelinde çöp öğeler | Tür ayrımı, İnceleme kutusu, geri alma, reddedilenlerin hatırlanması; gerekirse ayardan onay moduna geçiş (V2) |
| Bulanık blok eşleştirmenin hatalı eşleşmesi | Yinelenen ya da yanlış güncellenen öğe | Ayarlanabilir benzerlik eşiği, eşleştirme testleri, "kullanıcı kazanır" kuralı |
| Yapay zekâ özetlerinde hata birikimi | Yanlış yanıt ve rapor | Özetler yalnız yönlendirme için; bağlamda ve kaynaklarda ham notlar öncelikli (§6) |
| WebView2 RAM tüketimi | Boşta hedefin aşılması | Erken ölçüm; hedefin gerçek değere göre güncellenmesi |
| Kullanıcının CPU'sunun AVX2 desteklememesi | Model çalışmaz | Birden fazla llama.cpp derlemesi veya açılışta CPU kontrolü |
| Editör + bağlantı sisteminin büyüklüğü | Yapay zekâ işçisine geç gelinmesi | M0'da ince editör; wikilink/geri bağlantılar M3'te; hazır editör bileşeni (CodeMirror 6) |

---

## 12. Açık Kararlar

- [ ] Ürün adı ("İşletim Sistemi" ifadesi başlıktan çıkarıldı).
- [x] Frontend: **Svelte 5 + TypeScript** (Vite). *(2026-09-26)*
- [ ] llama.cpp entegrasyonu: sidecar (önerilen) mi, `llama-cpp-2` crate'i mi?
- [ ] LLM ve embedding modeli (değerlendirme setlerinden sonra); embedding modeli kurulumla mı gelsin?
- [ ] MVP'de GPU hızlandırması olacak mı?
- [x] Vault konumu ve klasör yapısı: `Belgeler/PLA Vault`, sistem klasörleri + serbest alan, diskte İngilizce adlar (§5.1). *(2026-09-26)*
- [x] `- [ ]` onay kutuları: MVP'de görev sayılmaz, iki yönlü eşitleme V2. *(2026-09-26)*
- [x] Veritabanı ayrımı: `pla.db` (kullanıcı verisi, vault'a yedeklenir) + `cache.db` (yeniden üretilebilir). *(2026-09-26)*
- [x] Günlük özetler vault'ta değil `cache.db`'de; vault'ta yalnız haftalık raporlar. *(2026-09-26)*
- [x] Bağlam penceresi 8k, bütçe paylaşımı §6'da. *(2026-09-26)*
- [ ] Varsayılan şablonların içeriği (günlük not, haftalık rapor).
- [ ] Metrik setinin, birimlerin ve birleştirme kurallarının kesinleşmesi (§5.4 taslak).
- [ ] Bulanık blok eşleştirme algoritması ve benzerlik eşiği.
- [ ] Soru-cevap geçmişi saklanacak mı, hafızaya (RAG) dahil edilecek mi?
- [x] Fine-tuning: MVP sonrası, yalnız taban ölçümü gerekli gösterirse; biçim değil anlamsal doğruluk için (§8.2). *(2026-09-26)*

---

## 13. Kilometre Taşları (öneri, tarihsiz)

İlke: Editör ince başlar; ürünün asıl değeri olan yapay zekâ işçisi erken doğrulanır.

| # | Çıktı |
|---|---|
| M0 | İskelet: Tauri + Svelte + Rust; `pla.db` / `cache.db` şemaları ve migrasyon altyapısı; **i18n altyapısı**; vault (klasör ağacı, temel CodeMirror editörü, kaydetme + dış değişiklik kontrolü) |
| M1 | Model indirme; `llama-server` yaşam döngüsü; süreç içi embedding; üç değerlendirme seti ve test düzeneğiyle model seçimi |
| M2 | Otonom çıkarım (blok eşleştirme, referans tarih, kullanıcı kazanır, İnceleme kutusu); görev paneli; hatırlatıcı bildirimleri ve temel tepsi modu; metrikler ve hızlı giriş |
| M3 | Not sistemi derinleşir: wikilink, geri bağlantılar, etiketler, FTS arama |
| M4 | RAG (`sqlite-vec`, hibrit arama, bağlam bütçesi); soru-cevap paneli ve araç çağırma |
| M5 | Günlük bakım (özetler, indeks, `pla.db` yedeği); haftalık rapor; telafi (açılış + uykudan uyanma); tepsi modunun tamamlanması |
| M6 | TR/EN çevirilerin tamamlanması, performans ölçümleri, kabul testleri |
| M7 *(MVP sonrası, koşullu)* | Çıkarım için fine-tuning: veri seti, QLoRA eğitimi, taban çizgisiyle karşılaştırma (§8.2) |
