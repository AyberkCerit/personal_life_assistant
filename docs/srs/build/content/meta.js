module.exports = {
  title: "PLA — Personal Life Assistant",
  subtitle: "Edge AI destekli, çevrimdışı çalışan yerel kişisel asistan",
  author: "Ayberk Cerit",
  headerText: "PLA · Yazılım Gereksinimleri Belirtimi · Sürüm 1.4",
  coverRows: [
    ["Belge", "Yazılım Gereksinimleri Belirtimi (SRS)"],
    ["Sürüm", "1.4"],
    ["Tarih", "16 Ekim 2026"],
    ["Hazırlayan", "Ayberk Cerit (ürün sahibi ve geliştirici)"],
    ["Dayanak", "Ürün Gereksinimleri Dokümanı (PRD) v0.4"],
    ["Standartlar", "ISO/IEC/IEEE 29148:2018 · EARS · ISO/IEC 25010"],
    ["Depo", "github.com/AyberkCerit/personal_life_assistant"],
  ],
  coverNote: "Bu belge PRD v0.4'teki kararları doğrulanabilir gereksinimlere çevirir. PRD'de açık kalan konularda alınan varsayılan kararlar Ek E'de listelenmiştir ve ürün sahibi tarafından onaylanmıştır.",
  history: [
    ["0.1", "26.09.2026", "Ayberk Cerit", "SRS hazırlık planı (docs/SRS_PLAN.md)."],
    ["1.0-taslak", "27.09.2026", "Ayberk Cerit (Claude ile)", "İlk tam taslak: 13 işlevsel modül, arayüz ve kalite gereksinimleri, 9 diyagram, veri modeli, izlenebilirlik matrisi, lisans tablosu. İnisiyatifle alınan kararlar Ek E'de onaya sunuldu."],
    ["1.1-taslak", "01.10.2026", "Ayberk Cerit (Claude ile)", "F1 model denemesi bulguları: dil modeli Gemma 4 E2B Q3_K_M seçildi; yeni tarih varsayılanları (FR-EXT-027, FR-EXT-028); düşük bellek sunucu ayarları (FR-MDL-021); çıkarım şemasında alan sırası (C.3); lisans tablosu (Gemma 4 Apache-2.0); eski belgeler depodan kaldırıldı."],
    ["1.2", "02.10.2026", "Ayberk Cerit (Claude ile)", "Ek E'deki 40 karar onaylandı; kod imzalama (NFR-SEC-009, D22) önceliği Orta → Düşük. Taslak ibaresi kaldırıldı."],
    ["1.3", "15.10.2026", "Ayberk Cerit (Claude ile)", "M6 performans ölçümleri (10 000 notluk referans kasa; model en düşük yapılandırmadaki gibi 3 thread): NFR-PERF-006 tepside ≤150 MB, pencere açıkken ≤450 MB (pencere tepsiye kapanınca WebView kapanır); NFR-PERF-009 10 bloklu not 10 s → 60 s (blok başına ~5 s, CPU'da üretim hızı sınırı); NFR-PERF-010 2/5 s → önceden okumadan sonra araçsız 45 s, araçlı 70 s (CPU'da prompt işleme ~50–65 token/s). Yeni: FR-EXT-030 (ilk açılışta yalnız son 30 günün notları çıkarılır), FR-QA-017 (soru kutusuna girilince önceden okuma); FR-EDT-004'e açılır-kapanır klasörler."],
    ["1.4", "16.10.2026", "Ayberk Cerit (Claude ile)", "M6 kabul denetimi: FR-BKP-004 bütünlüğü sağlam en yeni yedeği önerir, yanıt gelene kadar arka plan yapay zekâsı bekler; FR-LOG-007 bozuk pla.db'yi salt okunur açmak yerine silmeden ayrı tutar, notlar kullanılabilir kalır (sahibin onayı); FR-MDL-009/010 llama.cpp'nin çalışma anında CPU arka ucu seçmesine ve AVX2'siz temel x86-64 arka uca göre yazıldı; FR-MDL-016 embedding modelinin ayrı sunucu sürecinde çalışmasına (M4 kararı) göre düzeltildi."],
  ],
};
