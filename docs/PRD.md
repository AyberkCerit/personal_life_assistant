# Ürün Gereksinimleri Dokümanı (PRD)

| | |
|---|---|
| **Proje** | PLA — Edge AI Destekli Yerel Kişisel Asistan *(çalışma adı)* |
| **Yazar** | Ayberk Cerit |
| **Sürüm / Durum** | v0.2 — Taslak |
| **Tarih** | 2026-09-26 |
| **Platform (MVP)** | Masaüstü, Windows öncelikli (Tauri sayesinde macOS/Linux sonradan) |

### v0.1 → v0.2 değişiklikleri
- Kapsam netleşti: yalnız masaüstü, bireysel proje, çoklu dil hedefi.
- Yapay zekâ iki modda çalışır: **arka plan işçisi** + **soru-cevap / komut paneli**.
- Ayrı bir C++ katmanı yazılmıyor; llama.cpp hazır bir süreç (`llama-server`) olarak Rust'tan yönetiliyor.
- Hafıza mimarisinde ham veri **asla silinmez**; özetler yalnız ek bir katman.
- SQLite-vss / Chroma yerine `sqlite-vec`; embedding modeli bütçeye eklendi.
- Terim düzeltmesi: "Gizli Enjeksiyon / Prompt Injection" → **RAG ile bağlam ekleme** (prompt injection bir saldırı türüdür, bkz. §9).
- Sağlık/verimlilik metrikleri geri geldi: notlardan çıkarım + hızlı giriş paneli.
- Yeni bölümler: kapsam dışı, çoklu dil, başarı kriterleri, riskler, açık kararlar, kilometre taşları.

---

## 1. Ürün Vizyonu ve Kapsamı

**Amaç:** Kullanıcının notlarını, görevlerini ve sağlık/verimlilik verilerini tamamen çevrimdışı tutan; yapay zekâyı arka planda çalışan otonom bir veri işleyici ve istendiğinde soru yanıtlayıp komut uygulayan yerel bir asistan olarak konumlandıran kişisel üretkenlik uygulaması.

**Etkileşim modları**

| Mod | Ne yapar | Tetikleyici |
|---|---|---|
| Arka plan işçisi | Notlardan görev/hatırlatıcı/metrik çıkarır, gece bakımını ve periyodik raporları üretir | Not kaydı, zamanlayıcı |
| Soru-cevap / komut | Notlar ve veriler üzerinden soruları yanıtlar, görev ekleme gibi işlemleri uygular | Kullanıcı |

### Kapsam içi (MVP)
- %100 yerel çalışma; ağ yalnız ilk açılışta model indirmek için kullanılır (§9).
- Uygulamanın kendi vault'u: Markdown tabanlı, bağlantı ve etiket destekli not sistemi (§5.1).
- Otomatik görev / hatırlatıcı / metrik çıkarımı (§5.2).
- Sağlık ve verimlilik metrikleri (§5.3).
- Soru-cevap ve komut paneli (§5.4).
- Zamanlanmış işler, sistem tepsisi, kaçırılan işlerin telafisi (§5.5).
- Arayüzde en az Türkçe ve İngilizce; notlar herhangi bir dilde yazılabilir (§7).

### Kapsam dışı (MVP)
- Mobil platformlar.
- Bulut senkronizasyonu, çok cihaz, çok kullanıcı.
- Uygulama içi disk şifrelemesi (V2'de opsiyonel ayar).
- Vault'un dışarıdan (başka editörlerle) düzenlenmesinin resmi desteği.
- Giyilebilir cihaz entegrasyonu, sesli komut.
- Bulut LLM API'leri.

---

## 2. Hedef Kullanıcı

Birincil kullanıcı geliştiricinin kendisi; ürün kişisel kullanım üzerinden olgunlaştırılır. Genel hedef kitle: verisini buluta vermek istemeyen, not tutan ve rutinlerini takip eden bilgi çalışanları ve öğrenciler.

---

## 3. Teknoloji Yığını ve Sistem Mimarisi

| Katman | Teknoloji | Görev |
|---|---|---|
| Sunum | Tauri 2 (HTML/CSS/JS), Markdown editörü (ör. CodeMirror 6) | Koyu tema odaklı arayüz, editör, görev/metrik panelleri, soru-cevap paneli, sistem tepsisi |
| İş mantığı | Rust | Vault ve dosya işlemleri, SQLite, zamanlayıcı, çıkarım hattı, araç (tool) yürütme, model sürecinin yaşam döngüsü |
| Çıkarım | llama.cpp (`llama-server` alt süreci) | GGUF modelini CPU/GPU üzerinde çalıştırmak; gramer/JSON şemasıyla kısıtlı çıktı; embedding üretimi |
| Depolama | Vault klasörü (`.md`) + SQLite + `sqlite-vec` | Notlar (asıl kaynak), görevler, metrikler, iş kayıtları, vektör indeksi |

**Mimari kararlar**
- **C++ kodu yazılmaz.** llama.cpp hazır ikili olarak paketlenir ve Rust tarafından sidecar süreç olarak başlatılıp durdurulur. Süreci sonlandırmak modelin RAM'ini kesin olarak serbest bırakır ve "yükle → işle → boşalt" stratejisine birebir uyar. Alternatif: `llama-cpp-2` crate'i ile süreç içi bağlama (açık karar, §12).
- `llama-server` yalnız `127.0.0.1` üzerinde, rastgele portta ve her çalıştırmada üretilen bir API anahtarıyla dinler.
- **Asıl kaynak kuralı:** Notların asıl kaynağı `.md` dosyalarıdır. SQLite; görevler, metrikler, iş kayıtları, not indeksi ve vektörler için kullanılır. Not içeriğinin kopyası SQLite'ta asıl kaynak olarak tutulmaz.

```
[Tauri UI] ⇄ (IPC) ⇄ [Rust çekirdek] ──► vault/*.md
                           │  ├────────► SQLite (+ sqlite-vec)
                           │  └─ zamanlayıcı / iş kuyruğu
                           ▼
                 [llama-server alt süreci]  (isteğe bağlı başlatılır, işi bitince kapatılır)
```

---

## 4. Donanım ve Performans

Uygulama **isteğe bağlı yükleme** (on-demand load) stratejisiyle çalışır: model sürekli açık kalmaz.

**Minimum hedef donanım (MVP)**
- x86-64 CPU, 4 çekirdek, AVX2
- 8 GB RAM
- ~5 GB boş disk (model + uygulama + veri)
- GPU opsiyonel (Vulkan/CUDA hızlandırması bir açık karar, §12)

| Metrik | Hedef | Not |
|---|---|---|
| Boşta RAM | ≤ 150 MB | Tauri + Rust + WebView2 süreçlerinin toplamı. v0.1'deki 80–100 MB hedefi ölçümle doğrulanacak. |
| Aktif çıkarım RAM | LLM ≤ 2 GB, embedding modeli ≤ 300 MB | İki model aynı anda yüklenmez. |
| Model soğuk yükleme | ≤ 5 sn (SSD) | |
| Tek not çıkarımı | ≤ 10 sn (yalnız CPU) | Kullanıcıyı bloklamaz, arka planda çalışır. |
| Soru-cevap ilk token | ≤ 5 sn (model sıcakken ≤ 2 sn) | |
| Model tutma süresi | Son istekten 60 sn sonra kapatılır | Soru-cevap oturumunda her soruda yeniden yükleme olmasın diye. |

---

## 5. Temel Özellikler

### 5.1. Vault ve Markdown Not Sistemi
Obsidian benzeri, ancak uygulamanın kendi içinde inşa edilen bir not sistemi.
- Vault, uygulamanın veri dizinindeki bir klasördür; notlar düz `.md` dosyalarıdır ve dışarıdan okunabilir.
- YAML frontmatter (ör. `tags`, `created`, `updated`).
- `[[wikilink]]` bağlantıları, geri bağlantı (backlinks) paneli, `#etiket` desteği.
- Klasör ağacı, hızlı dosya açma, tam metin arama (SQLite FTS5).
- Uygulama açılışında dosya özetleri (hash) karşılaştırılarak indeks tazelenir. Canlı dosya izleyici (dış düzenleme desteği) V2'dedir.
- Otomatik oluşturulan içerik (raporlar, günlük özetler) de vault'a not olarak yazılır, örn. `Raporlar/2026-W39.md`.

### 5.2. Otonom Veri Çıkarımı (Structured Extraction)
Kullanıcının serbest metin olarak yazdığı notlardan görev, hatırlatıcı ve metrik çıkarılır ve **otomatik olarak** eklenir.

**Örnek**
- Metin: *"Yarın sabah 8'de kalkıp omuz çalışacağım, shoulder press hedefi."*
- Çıktı:
  ```json
  {"items": [{"type": "task", "title": "Omuz antrenmanı", "date": "2026-09-27", "time": "08:00",
              "details": "shoulder press", "source": {"note": "Günlük/2026-09-26.md", "block": "b3f1"}}]}
  ```
- Sonuç: Görev, panele "AI tarafından eklendi" rozetiyle düşer.

**Kurallar**
- **Tetikleme:** Not kaydedildiğinde (veya yazma 2 sn durduğunda) yalnız **değişen bloklar** işlenir.
- **Tarih bağlamı:** Modele bugünün tarihi, günü ve saat dilimi verilir. Göreli ifadeler ("yarın", "cuma") model çıktısında ISO tarihine dönüşür; Rust tarafı tarihi doğrular.
- **Kısıtlı çıktı:** JSON şeması / GBNF grameri kullanılır. Şemaya uymayan veya doğrulamayı geçmeyen çıktı eklenmez, "İnceleme" kutusuna düşer.
- **Tür ayrımı:** Model gelecekteki niyetleri (`task` / `reminder`) geçmiş kayıtlardan (`metric`, örn. "dün 7 saat uyudum") ayırır.
- **Tekrar önleme:** Her öğe kaynak bloğun hash'iyle ilişkilendirilir. Aynı blok yeniden işlendiğinde yeni öğe oluşmaz, var olan güncellenir; blok silinirse öğe "kaynağı silindi" olarak işaretlenir (silinmez).
- **Geri alma:** Her otomatik ekleme bir bildirim ve "Geri al" eylemiyle gösterilir; görevden kaynak nota tek tıkla gidilebilir.
- **Sınırlama:** Otomatik çıkarım yalnız **ekler/günceller**; hiçbir şeyi silmez.

### 5.3. Sağlık ve Verimlilik Metrikleri
- **İki giriş yolu:** (a) notlardan otomatik çıkarım (§5.2), (b) küçük bir hızlı giriş paneli.
- **MVP metrik seti (taslak):** uyku (saat), su, adım, antrenman (egzersiz, set, tekrar, ağırlık), kilo.
- Her kayıt; kaynağını (`manual` / `extracted`) ve varsa kaynak notu taşır.
- Basit görünüm: günlük/haftalık liste ve temel trend grafikleri.

### 5.4. Soru-Cevap ve Komut Paneli
Sohbet uygulamanın merkezi değildir; bir yan panel veya komut paleti olarak açılır.
- **Okuma:** Notlar (RAG, §6), görevler ve metrikler (yapılandırılmış sorgular) üzerinden yanıt verir. Örn. *"Geçen hafta ortalama kaç saat uyudum?"*, *"Proje X hakkında ne not almıştım?"*
- **İşlem:** Araç çağırma (tool calling) ile komut uygular. Araç seti (taslak): `search_notes`, `query_tasks`, `query_metrics`, `add_task`, `complete_task`, `log_metric`, `create_note`.
- **Kaynak gösterme:** Yanıtlar kullandığı notlara bağlantı verir. Yeterli bilgi yoksa "bulamadım" der, uydurmaz.
- **Onay kuralı:** Ekleme ve güncelleme doğrudan uygulanır ve geri alınabilir. **Silme** ve toplu değişiklikler açık onay ister.
- **Dil:** Yanıt, sorunun dilinde verilir.

### 5.5. Zamanlanmış İşler ve Arka Plan Çalışması
- **Rust zamanlayıcısı** şu işleri yürütür:
  - **Gece bakımı** (varsayılan 02:00): günlük özetlerin üretilmesi, embedding indeksinin güncellenmesi.
  - **Haftalık rapor** (varsayılan Pazar 20:00): tamamlanan görevler, metrikler ve notlardan "Haftalık Verimlilik ve Strateji Özeti" üretir ve vault'a not olarak yazar.
- **Tepsi modu:** Pencere kapatılınca uygulama sistem tepsisinde çalışmaya devam eder. Windows açılışında otomatik başlatma ayarlardan açılıp kapatılabilir.
- **Telafi:** Her işin son çalışma zamanı SQLite'ta tutulur. Açılışta kaçırılan işler, her iş türü için yalnız en son kaçırılan örnek olmak üzere çalıştırılır.
- **Nezaket:** Kullanıcı aktifken ağır işler ertelenebilir. Pil ve tam ekran algılama V2'dedir.

---

## 6. Hiyerarşik Hafıza Yönetimi

Amaç: Dil modelinin bağlam sınırını aşmadan ve RAM taşmasına (OOM) yol açmadan geçmiş bilgiye erişmek.

| Katman | İçerik | Saklama | Silinir mi? |
|---|---|---|---|
| 1. Ham veri | Notlar, görevler, metrikler | `.md` + SQLite | **Hayır.** Yalnız kullanıcı siler. |
| 2. Özetler | Günlük ve haftalık yoğunlaştırılmış özetler (kaynak bağlantılarıyla) | Vault'ta `.md` (+ SQLite indeks) | Yeniden üretilebilir |
| 3. Semantik indeks | Not ve özet parçalarının (chunk) embedding'leri | `sqlite-vec` | Yeniden üretilebilir |

- **Sıkıştırma** ham veriyi silmez; üst katmana özet ekler. Özet kaynağına bağlı olduğu için halüsinasyon durumunda asıl veri korunur ve doğrulanabilir.
- **Bağlam oluşturma (soru-cevap):** son 48 saatin ham verisi + en ilgili *k* parça (vektör + FTS hibrit arama) + ilgili yapılandırılmış sorgu sonuçları, sabit bir token bütçesi içinde (ör. 4k bağlam penceresi). Bu işleme **RAG ile bağlam ekleme** denir.
- Embedding için çok dilli küçük bir model kullanılır (ör. *multilingual-e5-small* sınıfı, GGUF).

---

## 7. Çoklu Dil

- **Arayüz:** i18n altyapısı baştan kurulur; MVP'de Türkçe ve İngilizce.
- **İçerik:** Notlar herhangi bir dilde olabilir. LLM ve embedding modeli çok dilli seçilir.
- **Model seçim ölçütü:** Türkçe + İngilizce en az 50 örneklik bir değerlendirme seti hazırlanır. Aday modeller (1.5–4B aralığı, Q4 kuantizasyon) şu ölçütlerle karşılaştırılır:
  - JSON geçerlilik oranı
  - Alan doğruluğu (tarih, saat, tür)
  - Araç çağırma doğruluğu
  - Hız
  - RAM
- Tarih ve sayı biçimleri arayüz diline göre gösterilir; veri her zaman ISO biçiminde saklanır.

---

## 8. Model Dağıtımı

- Kurulum paketi model içermez. **İlk açılışta** kullanıcıya önerilen model gösterilir ve açık onayıyla indirilir.
- İndirme sonrasında SHA-256 doğrulaması yapılır; indirme yarıda kalırsa devam ettirilebilir.
- İleri kullanıcı, diskteki kendi GGUF dosyasını seçebilir.
- İndirme tamamlandıktan sonra uygulama ağ erişimi olmadan tam işlevseldir.

---

## 9. Güvenlik ve Gizlilik

- **Sıfır telemetri:** Hiçbir kullanıcı verisi, alışkanlığı veya notu cihaz dışına çıkmaz. Otomatik güncelleme kontrolü MVP'de yoktur.
- **Ağ izolasyonu:** Tauri yetki (capability) listesi ağ erişimini yalnız model indirme adresiyle sınırlar. LLM çıkarımı ve vektör arama %100 yereldir.
- **Yerel sunucu güvenliği:** `llama-server` yalnız loopback adresinde, rastgele portta ve API anahtarıyla çalışır; iş bitince kapatılır.
- **Prompt injection'a karşı:** Notlar veya içe aktarılan metinler modele talimat gibi görünen içerik taşıyabilir. Bu nedenle:
  - Otomatik çıkarım hiçbir şeyi silemez.
  - Soru-cevaptaki silme ve toplu işlemler açık onay ister.
  - Modelin erişebildiği araçlar yukarıdaki sabit listeyle sınırlıdır; dosya sistemine veya ağa serbest erişimi yoktur.
- **Veri sahipliği:** Vault klasörü ve SQLite dosyası kullanıcınındır; yedekleme klasörü kopyalamakla yapılabilir. Dışa aktarma işlevi (görev ve metrikler için CSV/JSON) sağlanır.
- **Şifreleme:** MVP'de dosyalar açık formattadır. Disk şifrelemesi işletim sistemine bırakılır (BitLocker önerilir). Uygulama içi şifreleme V2'de opsiyonel ayar olarak gelir.

---

## 10. Başarı Kriterleri (MVP kabul ölçütleri)

1. **Çıkarım kalitesi:** Değerlendirme setinde JSON geçerliliği ≥ %95; tarih, saat ve tür alanlarında doğruluk ≥ %85 (TR ve EN ayrı ayrı).
2. **Kaynak:** Boşta ve çıkarım sırasında §4'teki RAM hedefleri ölçümle sağlanır.
3. **Veri güvenliği:** Hiçbir otomatik işlem ham veri silmez; buna ait bir test kapsamı vardır.
4. **Güvenilirlik:** Kapalı geçen bir Pazar'dan sonraki ilk açılışta haftalık rapor telafi edilerek üretilir.
5. **Soru-cevap:** Yanıtların kaynak gösterdiği ve bilgi yokken uydurmadığı bir test seti geçilir.
6. **Kullanım:** Geliştirici uygulamayı 4 hafta boyunca günlük asıl not/görev aracı olarak kullanır.

---

## 11. Riskler

| Risk | Etki | Önlem |
|---|---|---|
| Küçük modelin Türkçe ve analiz kalitesi düşük | Yanlış çıkarım, yüzeysel rapor | Değerlendirme seti ile model seçimi; kısıtlı gramer; model değiştirilebilir mimari |
| Küçük modelde araç çağırma güvenilir değil | Soru-cevap komutları hatalı | Gramerle kısıtlı araç çağrısı, sınırlı araç seti, geri alma |
| Otomatik eklemenin gürültü üretmesi | Görev panelinde çöp öğeler | Tür ayrımı, İnceleme kutusu, geri alma; gerekirse ayardan onay moduna geçiş (V2) |
| WebView2 RAM tüketimi | Boşta hedefin aşılması | Erken ölçüm; hedefin gerçek değere göre güncellenmesi |
| Kullanıcının CPU'sunun AVX2 desteklememesi | Model çalışmaz | Birden fazla llama.cpp derlemesi veya açılışta CPU kontrolü |
| Editör + bağlantı sisteminin büyüklüğü | Takvim kayması | Hazır editör bileşeni (CodeMirror 6); gelişmiş özellikleri V2'ye bırakma |

---

## 12. Açık Kararlar

- [ ] Ürün adı ("İşletim Sistemi" ifadesi başlıktan çıkarıldı).
- [ ] Frontend yaklaşımı: düz JS mi, Svelte mi, React mı?
- [ ] llama.cpp entegrasyonu: sidecar (önerilen) mi, `llama-cpp-2` crate'i mi?
- [ ] LLM ve embedding modeli (değerlendirme setinden sonra).
- [ ] MVP'de GPU hızlandırması olacak mı?
- [ ] Vault klasör yapısı ve varsayılan şablonlar (Günlük, Raporlar, Özetler…).
- [ ] Metrik setinin kesinleşmesi.
- [ ] Soru-cevap geçmişi saklanacak mı, hafızaya (RAG) dahil edilecek mi?
- [ ] Bağlam penceresi boyutu ve token bütçesi.

---

## 13. Kilometre Taşları (öneri, tarihsiz)

| # | Çıktı |
|---|---|
| M0 | İskelet: Tauri + Rust + SQLite; vault; editör; wikilink, etiket, FTS arama |
| M1 | Model indirme; `llama-server` yaşam döngüsü; değerlendirme seti ve model seçimi |
| M2 | Otonom çıkarım (görev, hatırlatıcı, metrik) + hızlı giriş paneli + görev/metrik görünümleri |
| M3 | Embedding + `sqlite-vec`; soru-cevap paneli ve araç çağırma |
| M4 | Zamanlayıcı, tepsi modu, telafi; gece bakımı ve haftalık rapor |
| M5 | i18n (TR/EN), performans ölçümleri, kabul testleri |
