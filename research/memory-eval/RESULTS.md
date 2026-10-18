# Asistan hafızası: okuma yolunun ölçümü (Aşama 1 kapısı)

Soru: hafıza dosyalarını (`memory/`) istemin içine koymak, notlarda olmayan ama hafızada olan soruları
cevaplatır mı ve notlardan bulunan bağlamı bozar mı?

## Nasıl çalıştırılır

```bash
$env:PLA_LLAMA_SERVER = "C:\dev\PLA\research\f1-model-eval\bin\llama-server.exe"
$env:PLA_EMBED_MODEL = "$env:LOCALAPPDATA\PLA\models\embeddinggemma-300M-Q8_0.gguf"
$env:PLA_MODEL = "C:\dev\PLA\research\f1-model-eval\models\gemma-4-E2B-it-Q3_K_M.gguf"
# arama (hızlı, ~1 dk)
cargo run --release -p pla-core --example retrieval_eval -- memory-p1
cargo run --release -p pla-core --example retrieval_eval -- memory-p1-nomem --no-memory
# asistan (model gerekir)
cargo run --release -p pla-core --example nl_eval -- <etiket> [id-öneki] [--no-memory]
```

`--no-memory` hafızayı dosyaya yazmaz ve yüklemez (boş `Recall`). `nl_eval` vakadaki `memory` alanını
geçici kasanın hafıza klasörüne `recall::render` ile yazar, `recall::load` ile yükler ve
`Question.recall`'a verir. `contains_any` ikinci bir "herhangi biri" grubudur; `contains` ile birlikte
verilirse ikisi de tutmalıdır.

## Dondurulan vakalar (commit `test(recall): freeze the memory eval cases`)

- `retrieval/vault.json`: 6 `none` vakası (IBAN, uçuş saati, tarif, vault'ta olmayan kişi, wifi şifresi,
  ehliyet sonucu; `facts: []`) ve 507 tokenlık bir `memory` fixture'ı (5 hakkımda, 3 kural, 3 durum,
  Deniz ve Efe için 3 kişi satırı). Fixture, mevcut 22 vakanın hiçbir olgusunu içermez.
- `nl/assistant.jsonl`: 8 geliştirme (`cat: memory`) ve 5 saklı (`cat: heldout`, id `memory-h-*`)
  vaka. Cevap yalnız hafızada: kişi olgusu (Deniz, Efe, Cenk), alışkanlık/saat (spor, laboratuvar günü,
  ulaşım, kahvaltı), alerji, telefon, kullanıcı kuralı (hitap, "Kolay gelsin") ve iki çıkarım durumu
  (`contains_any: görünüyor / notlarına göre / notlarında`). Uzunluk sınırı için düzenekte denetim
  olmadığından "kısa cevap" kuralı vakası yazılmadı. Saklı vakalar örnek bankasına Jaccard 0,5
  denetiminden geçti (düzenek başlangıçta denetler; koşu hata vermedi).

## Arama sonucu (EmbeddingGemma, `memory-p1c` ve `memory-p1c-nomem`)

Bütçe payları: son 48 saat + hafıza bloğu birlikte eski %40'ı oluşturur (`RECENT_TOKENS` + `MEMORY_TOKENS`,
her biri sabit); bulunan payı hafızadan önceki değerindedir, yalnız anılan kişilerin bloğu kadar küçülür.
Bu fixture'da hafıza bloğu 507 token.

| Mod | Hafızasız (Bağlı / Doğrudan) | Hafızalı (Bağlı / Doğrudan) | Ort. bulunan token (hafızasız → hafızalı) |
| --- | --- | --- | --- |
| kelime | 7/12 · 10/10 | 7/12 · 10/10 | 1143 → 1143 |
| kelime + bağ | 10/12 · 10/10 | 10/12 · 10/10 | 1328 → 1328 |
| hibrit | 10/12 · 10/10 | 10/12 · 10/10 | 1316 → 1316 |
| hibrit + bağ | 11/12 · 10/10 | 11/12 · 10/10 | 1472 → 1472 |

Hafızalı ve hafızasız koşu aynı: hibrit + bağ 21/22, hafıza bulunan notlara hiçbir şey kaybettirmiyor.
Doğrudan vakalarda gerileme yok. (`memory-p1.jsonl` ve `memory-p1-nomem.jsonl` dengelemeden önceki
sonuçlardır; güncel sonuç `memory-p1c*.jsonl`.)

### `none` vakaları (cevap vault'ta yok, 6 vaka)

| Mod | Ort. bulunan token | Ort. kaynak not |
| --- | --- | --- |
| kelime | 443 | 5,0 |
| kelime + bağ | 600 | 6,0 |
| hibrit | 1204 | 12,0 |
| hibrit + bağ | 1349 | 13,0 |

Hafızalı ve hafızasız aynı. Cevabı olmayan sorularda model hibrit modda yaklaşık 12 not ve 1200+ token okuyor.

### Geçmiş: dengelemenin nedeni

İlk uygulamada hafıza bloğu bulunan payın içinden harcanıyordu (`memory-p1`): hibrit + bağ **20/22**,
`linked-iha-ucuz` "6.800" olgusunu kaybetti, ort. bulunan token 1472 → 1341. Bu yüzden hafıza son 48
saat payının yanına, kendi diliminde alındı (karar L, düzeltme M); sonuç yukarıdaki tablo.

## Duman testi (gerçek model, tek vaka)

`memory-deniz-kahve`, Gemma 4 E2B Q3_K_M:

| Koşu | Sonuç | Cevap |
| --- | --- | --- |
| hafızalı | PASS (21,8 sn) | "Deniz kahveyi sütsüz ve şekersiz içer." |
| `--no-memory` | FAIL (19,9 sn): cevapta "sütsüz" yok | |

Düzenek hafızayı yazıyor, yüklüyor ve cevap yalnız hafızayla çıkıyor.

## Model koşuları (controller)

Doldurulacak:

- [ ] `nl_eval -- memory-p1 memory` ve `-- memory-p1-nomem memory --no-memory` (8 + 5 vaka; kapı: hafızasızda geçmeyen, hafızalıda geçen)
- [ ] Eski 122 vakada gerileme: `nl_eval -- memory-p1-all` ve `-- memory-p1-all-nomem --no-memory`
- [ ] `qa_probe -- <ref>` hafızalı ve hafızasız: ilk cevap süresi ve `cache_n`
