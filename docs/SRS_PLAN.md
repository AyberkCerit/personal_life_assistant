# SRS Hazırlık Planı

| | |
|---|---|
| **Belge** | PLA Yazılım Gereksinimleri Belirtimi (SRS) için hazırlık planı |
| **Dayanak** | [PRD v0.3](PRD.md) |
| **Durum** | Taslak — Ayberk'in incelemesinde |
| **Tarih** | 2026-09-26 |

---

## 1. Amaç ve PRD ile ilişki

PRD ürünün **neden** ve **ne** olduğunu anlatır: vizyon, kapsam, kararlar ve gerekçeleri. SRS ise bunu **doğrulanabilir gereksinimlere** çevirir: her madde tek bir davranış tanımlar, bir kimliği, önceliği ve nasıl doğrulanacağı vardır.

Kurallar:
- SRS gerekçe tekrarlamaz; gerektiğinde PRD bölümüne atıf verir (ör. "Kaynak: PRD §5.2").
- SRS yazılırken PRD'de bir boşluk veya çelişki çıkarsa önce PRD düzeltilir (PRD v0.4), SRS ona uyar. Tek doğru kaynak PRD'dir.
- SRS yalnız **MVP** gereksinimlerini içerir. V2 maddeleri ekte "gelecek kapsam" olarak listelenir, gereksinim yazılmaz.
- SRS **ne** yapılacağını söyler, **nasıl** yapılacağını değil. Örneğin bulanık eşleştirmenin algoritması SRS'te değil, tasarım belgesinde (ileride `docs/design/`) yer alır. SRS yalnız davranışı ve kabul ölçütünü tanımlar.

---

## 2. Dayanılan standartlar

- **ISO/IEC/IEEE 29148:2018** (eski IEEE 830'un yerini alan standart): bölüm yapısı ve iyi gereksinim özellikleri (tek anlamlı, doğrulanabilir, tutarlı, izlenebilir).
- **EARS** (Easy Approach to Requirements Syntax): gereksinim cümle kalıpları. Eski SRS'te de kullanılmıştı.
- **ISO/IEC 25010**: kalite (işlevsel olmayan) gereksinimlerin sınıflandırılması.

---

## 3. Gereksinim yazım kuralları

### 3.1. EARS kalıpları

| Tür | Kalıp | Örnek |
|---|---|---|
| Her zaman geçerli | The system shall … | The system shall store notes as plain `.md` files. |
| Olay güdümlü | WHEN <olay>, the system shall … | WHEN a note is closed, the system shall queue its changed blocks for extraction. |
| Durum güdümlü | WHILE <durum>, the system shall … | WHILE the Q&A panel is open, the system shall keep the LLM loaded. |
| İstenmeyen davranış | IF <koşul>, THEN the system shall … | IF the note file was modified externally, THEN the system shall not overwrite it. |
| Opsiyonel özellik | WHERE <özellik>, the system shall … | WHERE autostart is enabled, the system shall start minimized to tray. |
| Karmaşık | Yukarıdakilerin birleşimi | WHILE in tray mode, WHEN a reminder is due, the system shall … |

### 3.2. Kimlik şeması

`<TÜR>-<ALAN>-<NO>`, örn. `FR-EXT-012`.

| Tür | Anlamı |
|---|---|
| `FR` | İşlevsel gereksinim |
| `NFR` | Kalite gereksinimi |
| `IR` | Arayüz gereksinimi |
| `DR` | Veri gereksinimi |
| `CON` | Kısıt |

Numaralar ardışık gider ve **yeniden kullanılmaz**. Silinen gereksinim "Kaldırıldı" olarak işaretlenir, numarası boş kalır.

### 3.3. Her gereksinimin alanları

| Alan | Değerler |
|---|---|
| Kimlik | `FR-EXT-012` |
| Gereksinim | EARS cümlesi; tek bir "shall" |
| Öncelik | `Must` (MVP'nin olmazsa olmazı) / `Should` (MVP, gerekirse ertelenebilir) |
| Doğrulama | `T` test · `E` değerlendirme seti · `I` inceleme · `D` gösterim · `A` analiz/ölçüm |
| Kaynak | PRD bölümü, ör. `PRD §5.2` |
| Kilometre taşı | M0–M6 |
| Not | İsteğe bağlı; kabul ölçütü veya açıklama |

### 3.4. Kalite kontrol listesi (her gereksinim için)
- Tek bir davranış mı anlatıyor?
- "Hızlı", "kolay", "kullanıcı dostu" gibi ölçülemeyen kelimeler yok mu? Varsa sayıya çevrildi mi?
- Nasıl test edileceği belli mi?
- Başka bir gereksinimle çelişiyor mu?
- PRD'ye izlenebiliyor mu?

---

## 4. SRS belge yapısı (önerilen içindekiler)

| # | Bölüm | İçerik | Tahmini gereksinim |
|---|---|---|---|
| 1 | Giriş | Amaç, kapsam, belge düzeni, **terimler sözlüğü** (vault, blok, `block_id`, öğe, çıkarım, referans tarih, İnceleme kutusu, günlük bakım, `pla.db`, `cache.db` …), referanslar, yazım kuralları (bu planın 3. bölümü) | — |
| 2 | Genel açıklama | Ürün perspektifi (bağlam diyagramı), işlev özeti, kullanıcı sınıfları, işletim ortamı (Windows 10/11, WebView2, min. donanım), tasarım kısıtları, varsayımlar ve bağımlılıklar | ~10 `CON` |
| 3 | İşlevsel gereksinimler | Aşağıdaki 4.1 tablosundaki alanlar; her alan için açıklama, uyaran/yanıt akışı, gereksinimler, hata durumları | ~130–170 `FR` |
| 4 | Harici arayüz gereksinimleri | Kullanıcı arayüzü (ekran listesi ve ekran başına zorunlu öğeler; tel kafes çizim yok), `llama-server` HTTP API kullanımı, Windows bildirimleri, dosya sistemi, **Tauri IPC komut sözleşmesi**, dosya biçimleri (frontmatter anahtarları, `.pla/config` şeması) | ~25 `IR` |
| 5 | Veri gereksinimleri | `pla.db` ve `cache.db` mantıksal veri modeli (ER), **çıkarım JSON şeması (sürümlü)**, araç çağırma şemaları, veri yaşam döngüsü, şema migrasyonu, yedek biçimi | ~25 `DR` |
| 6 | Kalite gereksinimleri | ISO 25010'a göre: performans, güvenilirlik ve veri bütünlüğü, güvenlik ve gizlilik, kullanılabilirlik, erişilebilirlik, bakım yapılabilirlik (log, migrasyon), taşınabilirlik, **yapay zekâ kalitesi** (değerlendirme eşikleri) | ~35 `NFR` |
| 7 | Doğrulama | Test seviyeleri, değerlendirme setlerinin tanımı ve saklanma yeri, PRD §10 kabul ölçütlerinin gereksinimlere eşlenmesi | — |
| Ek A | İzlenebilirlik matrisi | PRD bölümü → SRS kimlikleri → doğrulama yöntemi / test | — |
| Ek B | Diyagramlar | Aşağıdaki 4.2 listesi | — |
| Ek C | Açık konular (TBD) | Karar bekleyen maddeler ve etkiledikleri gereksinimler | — |
| Ek D | Gelecek kapsam (V2) | Gereksinim yazılmayan maddeler | — |

**Toplam tahmin:** 220–260 gereksinim. Tek kişilik bir proje için büyük görünebilir, ama çoğu kısa ve M0–M2'de doğrudan test senaryosuna dönüşecek.

### 4.1. İşlevsel alanlar (Bölüm 3)

| Kod | Alan | PRD | İlk kilometre taşı | Kapsadığı başlıca konular |
|---|---|---|---|---|
| `SET` | İlk açılış ve ayarlar | — *(PRD'de eksik)* | M0 | İlk açılış akışı (dil, vault konumu, model indirme onayı), ayarlar ekranı, otomatik başlatma |
| `VLT` | Vault ve dosya işlemleri | §5.1 | M0 | Oluşturma ve açma, sistem klasörleri, `.pla/config`, Obsidian sözdizimi kısıtları, dış değişiklik kontrolü, atomik yazma, indeksleme |
| `EDT` | Editör ve not özellikleri | §5.1 | M0 / M3 | CodeMirror, frontmatter, wikilink, geri bağlantılar, etiketler, FTS arama, hızlı açma |
| `I18N` | Çoklu dil | §7 | M0 | Arayüz dili, yerelleştirilmiş klasör adları, tarih ve sayı biçimleri |
| `MDL` | Model yönetimi | §3, §8.1 | M1 | İndirme, SHA-256, devam ettirme, özel GGUF, `llama-server` yaşam döngüsü, 60 sn tutma, AVX2 kontrolü, embedding |
| `EXT` | Otonom çıkarım | §5.2 | M2 | Kapsam, tetikleme, blok eşleştirme, referans tarih, şema doğrulama, kullanıcı kazanır, reddedilenler, geri alma |
| `TSK` | Görevler ve hatırlatıcılar | §5.3 | M2 | Görev paneli ve işlemleri, İnceleme kutusu, bildirimler, kaçırılan hatırlatıcılar |
| `MET` | Metrikler | §5.4 | M2 | Hızlı giriş, birleştirme kuralları, çakışma, grafikler, istatistikler |
| `MEM` | Hafıza ve RAG | §6 | M4 | Parçalama (chunking), embedding, hibrit arama, bağlam bütçesi, özetler |
| `QA` | Soru-cevap ve araçlar | §5.5 | M4 | Panel, araçlar, onay kuralı, kaynak gösterme, "bulamadım" davranışı |
| `SCH` | Zamanlayıcı ve arka plan | §5.6 | M2 / M5 | Tepsi modu, günlük bakım ve boşta koşulu, haftalık rapor, telafi, uykudan uyanma |
| `BKP` | Yedek ve dışa aktarma | §5.1, §9 | M5 | `pla.db` yedeği ve 7 kopya kuralı, CSV/JSON dışa aktarma, geri yükleme |

### 4.2. Diyagramlar (Ek B)
`diagram-design` ile HTML + PNG olarak `docs/srs/diagrams/` altına çizilir; SRS içinde gömülür.

| # | Diyagram | Tür | Neden gerekli |
|---|---|---|---|
| 1 | Sistem bağlamı | Bağlam / mimari | Sistem sınırı: kullanıcı, işletim sistemi, dosya sistemi, model sunucusu, indirme adresi |
| 2 | Bileşen mimarisi | Mimari | Svelte ↔ IPC ↔ Rust modülleri ↔ veritabanları ↔ `llama-server` |
| 3 | Kullanım senaryoları | Use case | Aktörler ve MVP senaryoları (eski SRS'teki use case'in yenisi) |
| 4 | Çıkarım hattı | Sıralama (sequence) | Not kapanışı → blok eşleştirme → model → doğrulama → kayıt → bildirim |
| 5 | Soru-cevap ve araç çağırma | Sıralama | Soru → embedding → hibrit arama → bağlam → model → araç → yanıt |
| 6 | Günlük bakım ve telafi | Akış | Boşta koşulu, uykudan uyanma, kaçırılan işler |
| 7 | Çıkarılan öğenin yaşam döngüsü | Durum makinesi | Oluşturuldu → kullanıcı değiştirdi / reddedildi / kaynağı silindi … |
| 8 | Model süreci yaşam döngüsü | Durum makinesi | Kapalı → yükleniyor → sıcak → 60 sn → kapatılıyor |
| 9 | `pla.db` / `cache.db` veri modeli | ER | Tablolar ve ilişkiler |

---

## 5. SRS'i başlatmadan önce netleşmesi gerekenler

### 5.1. SRS'in biçimine dair kararlar (senin kararın)

| # | Soru | Seçenekler | Önerim |
|---|---|---|---|
| K1 | Dil | (a) Belge Türkçe, EARS cümleleri İngilizce · (b) Tamamen Türkçe (EARS kalıpları Türkçeye uyarlanır) · (c) Tamamen İngilizce | **(a)**. Eski SRS'le uyumlu; EARS anahtar kelimeleri İngilizcede standart ve test adlarına doğrudan dönüşür. |
| K2 | Dosya yapısı | (a) Tek büyük `SRS.md` · (b) `docs/srs/` altında bölüm başına bir dosya + `README.md` içindekiler | **(b)**. 250 gereksinimlik tek dosya incelemesi ve farkları okumak zor. |
| K3 | Gereksinim biçimi | (a) Her alan için bir tablo · (b) Her gereksinim ayrı bir başlık bloğu | **(a)**. Kısa, taranabilir ve izlenebilirlik matrisine kolay dönüşür. |
| K4 | Kapsam derinliği | (a) Yalnız MVP · (b) MVP + V2 gereksinimleri | **(a)**. V2 yalnız Ek D'de listelenir. |

### 5.2. Gereksinimleri engelleyen PRD açık kararları

| PRD açık kararı | Etkilediği SRS bölümü | Engelliyor mu? | Önerim |
|---|---|---|---|
| Sidecar mı, `llama-cpp-2` mi? | `MDL`, Bölüm 4 (arayüzler) | **Evet** | Sidecar (PRD'nin önerisi); SRS'ten önce kapatalım |
| Metrik seti, birimler, kurallar | `MET`, Bölüm 5 (veri modeli) | **Evet** | PRD §5.4'teki taslağı onaylayıp kapatalım |
| Soru-cevap geçmişi saklanacak mı, RAG'e girecek mi? | `QA`, `MEM`, veri modeli | **Evet** | Sakla (`pla.db`), MVP'de RAG'e sokma, kullanıcı silebilsin |
| LLM / embedding modeli | — | Hayır | Gereksinimler modelden bağımsız yazılır |
| GPU hızlandırması | `NFR` | Hayır | TBD olarak işaretlenir |
| Blok eşleştirme algoritması ve eşiği | — | Hayır | Tasarım konusu; SRS yalnız davranışı tanımlar |
| Şablon içerikleri | `VLT` | Hayır | TBD |
| Ürün adı | — | Hayır | "PLA" kullanılır |

### 5.3. PRD'de hiç ele alınmamış konular (SRS yazılırken sorulacak)
Bunlar PRD v0.4'e eklenmeli:
1. **İlk açılış akışı:** dil seçimi → vault konumu → model indirme onayı → örnek not. Model indirilmeden uygulama kullanılabilir mi? (Önerim: evet; yapay zekâ özellikleri kapalı görünür.)
2. **Aynı anda birden fazla vault** olabilir mi? (Önerim: MVP'de tek vault, değiştirilebilir.)
3. **Uygulama güncellemesi:** Otomatik kontrol yok; peki kullanıcı yeni sürümü nasıl alacak? (Önerim: GitHub Releases'ten elle indirme; veritabanı migrasyonu açılışta otomatik.)
4. **Kaldırma:** Uygulama kaldırılınca `%APPDATA%/PLA` silinsin mi, vault'a asla dokunulmasın mı?
5. **Hata yönetimi ve log:** Log düzeyi, log dosyası boyut sınırı, loglara not içeriği yazılmaması (gizlilik).
6. **Erişilebilirlik hedefi:** Tam klavye kullanımı ve WCAG AA kontrastı yeterli mi?
7. **Çökmeye dayanıklılık:** Yazma sırasında elektrik kesilirse not ve veritabanı bozulmamalı. (Önerim: atomik dosya yazma + SQLite WAL.)

---

## 6. Yazım aşamaları

Her aşama: ben yazarım → sen incelersin → düzeltme → commit. Aşamalar kilometre taşı sırasını izler; böylece M0 kodlamasına SRS bitmeden, M0 bölümleri hazır olunca başlanabilir.

| Aşama | Çıktı | Ön koşul |
|---|---|---|
| **A0** | Bu plandaki K1–K4 kararları; 5.2'deki üç engelleyici karar; 5.3'teki sorular → **PRD v0.4** | Senin incelemen |
| **A1** | İskelet (`docs/srs/` klasör yapısı), Bölüm 1 (sözlük dahil) ve Bölüm 2; diyagram 1–3 | A0 |
| **A2** | **Bölüm 5: Veri gereksinimleri**: `pla.db` / `cache.db` modeli, çıkarım JSON şeması v1, araç şemaları; diyagram 9 | A1. Birçok FR buna dayandığı için öne alındı. |
| **A3** | M0 alanları: `SET`, `VLT`, `EDT` (temel), `I18N` | A2 |
| **A4** | M1–M2 alanları: `MDL`, `EXT`, `TSK`, `MET`; diyagram 4, 7, 8 | A3 |
| **A5** | M3–M5 alanları: `EDT` (ileri), `MEM`, `QA`, `SCH`, `BKP`; diyagram 5, 6 | A4 |
| **A6** | Bölüm 4 (arayüzler: IPC sözleşmesi, `llama-server`, dosya biçimleri) + Bölüm 6 (kalite gereksinimleri) | A5 |
| **A7** | Bölüm 7 (doğrulama), Ek A (izlenebilirlik matrisi), Ek C–D; tutarlılık turu (SRS ↔ PRD) → **SRS v1.0** | A6 |

**Pratik öneri:** A3 bitince M0 kodlamasına başlanabilir; A4–A7 kodlamayla paralel ilerler. SRS'in tamamını bitirmeyi beklemek, belgenin kodla hiç karşılaşmadan eskimesine yol açar.

---

## 7. Tamamlanma ölçütleri (SRS v1.0)

- [ ] PRD'nin her MVP maddesi en az bir gereksinime izlenebiliyor (Ek A).
- [ ] Her gereksinimin önceliği, doğrulama yöntemi ve kilometre taşı var.
- [ ] PRD §10'daki her kabul ölçütü en az bir gereksinime eşlenmiş.
- [ ] "Hızlı", "kolay", "uygun" gibi ölçülemeyen ifade kalmamış.
- [ ] Ek C'de engelleyici TBD kalmamış.
- [ ] 9 diyagram çizilmiş ve SRS'e gömülmüş.
