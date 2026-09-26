# SRS — İnisiyatif Kararları Günlüğü

SRS v1.0 yazılırken Ayberk'e sorulmadan, inisiyatifle alınan kararlar. Her biri onay bekler; reddedilen karar SRS'te ve gerekirse PRD'de düzeltilir.

Durum: ⏳ onay bekliyor · ✅ onaylandı · ❌ reddedildi

## A. Belge biçimi

| # | Karar | Gerekçe | Durum |
|---|---|---|---|
| A1 | Belge Türkçe; gereksinim cümleleri İngilizce EARS kalıbıyla (WHEN/WHILE/IF/WHERE … the system shall …). | EARS anahtar kelimeleri İngilizcede standart; eski SRS de böyleydi; test adlarına doğrudan dönüşür. SRS_PLAN K1-(a). | ⏳ |
| A2 | Tek `.docx` dosyası (`docs/SRS.docx`); içerik `docs/srs/build/` altındaki üretici betikten oluşturulur. | Ayberk Word formatı istedi; betikle üretmek sürümlemeyi ve güncellemeyi kolaylaştırır. | ⏳ |
| A3 | Gereksinimler her modülde tablo olarak: Kimlik, EARS şablonu, gereksinim, öncelik, doğrulama, kaynak. | Taranabilir, izlenebilirlik matrisine kolay dönüşür. SRS_PLAN K3. | ⏳ |
| A4 | Öncelik ölçeği: Yüksek = MVP'de zorunlu, Orta = MVP'de olmalı ama ertelenebilir, Düşük = varsa iyi olur. | Ayberk'in şablonundaki Yüksek/Orta/Düşük ölçeği MVP kapsamına eşlendi. | ⏳ |
| A5 | Yalnız MVP gereksinimleri yazıldı; V2 maddeleri ekte listelendi. | SRS_PLAN K4. | ⏳ |
| A6 | Şablona "1.6 Gereksinim Gösterimi ve EARS Şablonları" alt bölümü ve veri modeli, izlenebilirlik, açık konular, inisiyatif kararları ekleri eklendi. | EARS kullanımının ve veri modelinin belgede tanımlı olması gerekiyor; şablonun "Ekler" bölümü genişletildi. | ⏳ |

## B. PRD'de açık olan kararlar

| # | Karar | Gerekçe | Durum |
|---|---|---|---|
| B1 | llama.cpp **sidecar** (`llama-server` alt süreci) olarak kullanılır. | PRD'nin önerisi; süreci kapatmak RAM'i kesin boşaltır. | ⏳ |
| B2 | MVP metrik seti PRD §5.4 taslağı olarak kesinleşti: uyku (saat), kilo (kg), adım, su (ml), antrenman (egzersiz/set/tekrar/kg). | Engelleyici karardı; taslak makul. | ⏳ |
| B3 | Soru-cevap geçmişi `pla.db`'de saklanır, MVP'de RAG'e girmez, kullanıcı tamamen silebilir. | Engelleyici karardı; gizlilik ve hata birikimi riski düşük tutuldu. | ⏳ |
| B4 | Ürün adı belgede "PLA (Personal Life Assistant)" olarak kullanıldı. | Ürün adı henüz seçilmedi. | ⏳ |
| B5 | GPU hızlandırması MVP'de "Düşük" öncelikli opsiyonel özellik (varsayılan kapalı). | PRD'de açık; performans hedefleri yalnız CPU'ya göre. | ⏳ |

## C. PRD'de hiç ele alınmamış konular

| # | Karar | Gerekçe | Durum |
|---|---|---|---|
| C1 | İlk açılış sihirbazı: dil → vault konumu → model indirme (atlanabilir) → tamam. Model yokken yapay zekâ dışı her şey çalışır. | SRS_PLAN §5.3-1 önerisi. | ⏳ |
| C2 | Aynı anda tek aktif vault; ayarlardan değiştirilebilir. | SRS_PLAN §5.3-2 önerisi. | ⏳ |
| C3 | Uygulama güncellemesi GitHub Releases'ten elle; veritabanı migrasyonu açılışta otomatik ve önce yedek alınarak. | Otomatik güncelleme kontrolü PRD'de yok. | ⏳ |
| C4 | Kaldırmada vault'a asla dokunulmaz; `%APPDATA%/PLA` silinip silinmeyeceği kaldırıcıda sorulur. | Kullanıcı verisini koruma ilkesi. | ⏳ |
| C5 | Log: `%APPDATA%/PLA/logs`, 5 MB × 5 dosya döngü; loglarda not içeriği, görev başlığı, metrik değeri, soru-cevap metni yok. Hata ayıklama modu 24 saat sonra kendiliğinden kapanır. | Gizlilik + destek ihtiyacı. | ⏳ |
| C6 | Erişilebilirlik hedefi: tüm işlevler klavyeyle, WCAG 2.1 AA kontrastı. | SRS_PLAN §5.3-6 önerisi. | ⏳ |
| C7 | Çökmeye dayanıklılık: atomik dosya yazma (geçici dosya + yeniden adlandırma), SQLite WAL, anormal kapanıştan sonra bütünlük kontrolü. | SRS_PLAN §5.3-7 önerisi. | ⏳ |

## D. SRS yazılırken çıkan yeni kararlar

| # | Karar | Gerekçe | Durum |
|---|---|---|---|
| D1 | **`pla.db` ve `cache.db` vault başınadır:** `%APPDATA%/PLA/vaults/<vault_id>/`. `vault_id` `.pla/config`'te tutulur. Uygulama ayarları ayrı: `%APPDATA%/PLA/settings.json`. | PRD tek bir `pla.db` varsayıyordu; vault değiştirilince eski vault'un görevleri yenisine karışırdı. **PRD'ye yansıtılmalı (v0.4).** | ⏳ |
| D2 | Not silme işletim sisteminin Geri Dönüşüm Kutusu'na taşıma olarak yapılır (onayla). | "Ham veri kalıcı silinmez" ilkesiyle uyumlu, geri alınabilir. | ⏳ |
| D3 | Asistan (soru-cevap) MVP'de hiçbir silme aracına ve toplu değişiklik aracına sahip değil. | PRD "silme açık onay ister" diyordu; araç listesinde silme yok, en güvenli yol hiç sunmamak. | ⏳ |
| D4 | Asistanın `create_note` ile oluşturduğu notlar `pla_generated: true` taşır ve çıkarıma girmez. | Asistan zaten `add_task` ile görev ekleyebiliyor; aynı notun ayrıca çıkarımdan geçmesi yinelenen görev üretirdi. | ⏳ |
| D5 | Hafta Pazartesi başlar (ISO 8601); haftalık rapor Pazartesi–Pazar haftasını kapsar ve `YYYY-Www` adıyla yazılır. | Türkiye ve ISO uygulaması. | ⏳ |
| D6 | Haftalık rapor dosyası zaten varsa yeniden üretilmez, üzerine yazılmaz. | Kullanıcı raporu düzenlemiş olabilir. | ⏳ |
| D7 | 1 bardak su = 250 ml (ayarlanabilir); lb → kg dönüştürülür. | Metrik birimlerini tek biçimde saklamak için. | ⏳ |
| D8 | Metrik makul aralıkları: uyku 0–24 saat, kilo 20–400 kg, adım 0–100 000, su 0–10 000 ml/kayıt. Dışındaki değer çıkarımda İnceleme kutusuna düşer, elle girişte onay istenir. | Anlamsal doğrulama için somut sınır gerekiyor. | ⏳ |
| D9 | Hatırlatıcıda erteleme süresi 10 dakika; saatli hatırlatıcının bildirim zamanı varsayılan olarak tam o saat. | Basit, yaygın varsayılan. | ⏳ |
| D10 | Otomatik kaydetme: yazma 2 sn durunca veya not odağı kaybedince. (Çıkarım tetiklemesinden ayrıdır: 45 sn / not kapanışı.) | Veri kaybını önler; çıkarım yükünü artırmaz. | ⏳ |
| D11 | Parçalama (chunk) boyutu 200–400 token, başlık/paragraf sınırında. | RAG için yaygın ve 8k bağlama uygun. | ⏳ |
| D12 | Performans hedefleri eklendi: uygulama açılışı ≤ 3 sn, not açma ≤ 200 ms (≤ 1 MB not), arama ≤ 300 ms (10 000 not), tuş gecikmesi ≤ 16 ms, UI sorguları ≤ 100 ms. | Şablon ekran/sorgu süreleri istiyor; PRD'de yoktu. | ⏳ |
| D13 | Ölçek hedefi: 10 000 not / 1 GB vault, 5 yıllık görev ve metrik verisi. | Performans hedeflerinin ölçüleceği referans büyüklük. | ⏳ |
| D14 | Kimlik doğrulama yok: tek kullanıcılı yerel uygulama, işletim sistemi hesabına güvenir. | Yerel, tek kullanıcı. Şifreleme V2 ile birlikte uygulama kilidi düşünülebilir. | ⏳ |
| D15 | Uygulama içine yalnız izin verici lisanslı (MIT, Apache-2.0, BSD, Public Domain) bileşenler alınır; model lisansı indirmeden önce gösterilir. | Lisans uyumluluğu ve olası ticari kullanım. | ⏳ |
| D16 | Model indirme adresi izin listesi: `huggingface.co` (ve onun CDN alanı). | En yaygın GGUF kaynağı. | ⏳ |
| D17 | Ağ sürücüsündeki (UNC) vault desteklenmez; yerel disk veya bulut senkron klasörü desteklenir. | Kilit ve atomik yazma güvenilirliği. | ⏳ |
| D18 | Rapor ve özetler arayüz dilinde yazılır. | Çok dilli kullanımda tutarlılık. | ⏳ |
| D19 | Araç çağırma değerlendirme eşiği ≥ %80 doğru araç + doğru parametre. | PRD'de araç çağırma için eşik yoktu. | ⏳ |
| D20 | Çekirdek Rust modüllerinde birim test kapsamı hedefi ≥ %70. | Sürdürülebilirlik ölçütü için somut hedef. | ⏳ |
| D21 | Grafik kütüphanesi uPlot (MIT), i18n kütüphanesi svelte-i18n (MIT), embedding için fastembed-rs (Apache-2.0) + ONNX Runtime (MIT), çöp kutusu için `trash` crate (MIT). | Lisans tablosu somut bileşen adı istiyor; seçimler değiştirilebilir. | ⏳ |
| D22 | Kurulum paketi imzalanması (code signing) "Orta" öncelik. | SmartScreen uyarısını azaltır; bireysel projede maliyetli olabilir. | ⏳ |
