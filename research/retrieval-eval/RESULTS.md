# Bağlantı genişletmesi: arama değerlendirmesi (2026-10-18)

Soru: asistanın bağlamına, bulunan en iyi notların `[[bağlantı]]` verdiği notları da eklemek, cevabın
bağlama girme oranını artırır mı?

```bash
$env:PLA_LLAMA_SERVER = "C:\dev\PLA\research\f1-model-eval\bin\llama-server.exe"
$env:PLA_EMBED_MODEL = "$env:LOCALAPPDATA\PLA\models\embeddinggemma-300M-Q8_0.gguf"
cargo run --release -p pla-core --example retrieval_eval -- <etiket>
```

Düzenek uygulamanın kendi `qa::context::build_with` fonksiyonunu çalıştırır ve bir vakadaki her olgunun
modelin okuyacağı metinde (son 48 saat + bulunan parçalar) olup olmadığına bakar. Model cevabı ölçülmez,
yalnız cevabın bağlama girip girmediği ölçülür. Embedding modeli gerçek EmbeddingGemma 300M Q8_0'dır.

Vault ve vakalar `crates/pla-core/tests/retrieval/vault.json` dosyasında: 36 elle yazılmış not (projeler,
İHA seçimi, dersler, kişiler, spor, bütçe, dikkat dağıtıcılar) ve kişisel bir vault'taki gibi aynı
konulardan söz edip aynı merkez notlara bağlanan ama hiçbir olguyu içermeyen 300 üretilmiş günlük not.
22 vaka var. **Bağlı** (12): cevap, sorunun adını verdiği notun bağlandığı bir notta. **Doğrudan** (10):
cevap, sorunun adını verdiği notta.

## Sonuç

| Mod | Bağlı | Doğrudan | Toplam | Ort. bulunan token | Ort. süre |
| --- | --- | --- | --- | --- | --- |
| kelime | 7/12 | 10/10 | 17/22 | 1143 | 0.8 ms |
| kelime + bağ | **10/12** | 10/10 | 20/22 | 1328 | 1.3 ms |
| hibrit | 10/12 | 10/10 | 20/22 | 1316 | 2.5 ms |
| hibrit + bağ | **11/12** | 10/10 | 21/22 | 1472 | 2.9 ms |

Bağ genişletmesi doğrudan vakaların hiçbirini bozmadı. Bağlı vakalarda kelime aramasında +3, hibrit
aramada +1 kazandırdı. Maliyeti soru başına yaklaşık 150–185 token (bulunan payın %8–9'u) ve 1 ms'den az.
Embedding modeli indirilmeden önce (yalnız kelime araması) kazanç en büyük.

## Denenenler (`results/`)

| Etiket | Ayar | Bulgu |
| --- | --- | --- |
| `v1` | yalnız 36 not | Ölçüm değil: vault küçük, her şey bağlama sığıyor, temel arama zaten 22/22'ye yakın. |
| `v2-daily` | + 300 günlük; giden **ve gelen** bağlar, 2 tohum | Hiç kazanç yok. Merkez notların geri bağlantıları yüzlerce günlük; soruya benzedikleri için cevabı tutan notun yerini aldılar. |
| `v3-out-s2` | yalnız **giden** bağlar, 2 tohum | +3 / +1. Seçilen ayar. |
| `v3-out-s4` | yalnız giden bağlar, 4 tohum | Ek kazanç yok, ~100 token fazla. |
| `final` | seçilen ayar, sabitlenmiş kod | `v3-out-s2` ile aynı. |

## Kalan hatalar

- `linked-iha-rapor`: cevap iki adım uzakta (soru "bitirme projesi danışmanı", cevap danışmanın notunda;
  proje notu üretilmiş günlükler yüzünden ilk iki sonuca giremiyor). İki adımlık genişletme denenmedi.
- `linked-pla-yayin` (yalnız kelime): yine iki adım (günlük → PLA → Yol Haritası). Hibrit arama buluyor.

## Okurken dikkat

Vakaları, üretilmiş günlükleri ve algoritmayı aynı kişi (Claude) yazdı. Vakalar algoritmadan önce yazıldı,
ama vault'un bağlantı yapısı gerçek bir vault'tan daha düzenli olabilir. Asıl doğrulama gerçek bir vault
üzerinde, gerçek sorularla yapılmalı. Asistan değerlendirmesinin (`research/nl-eval`) vakalarında hiçbir
notta `[[bağlantı]]` olmadığı için genişletme onların bağlamını değiştirmez; o koşu tekrarlanmadı.
