# SRS kaynakları

`docs/SRS.docx` bu klasördeki kaynaklardan üretilir; Word dosyasını elle düzenlemek yerine içeriği buradan güncelleyin.

| Yol | İçerik |
|---|---|
| `build/content/*.js` | SRS içeriği (bölüm başına bir modül; gereksinim tabloları EARS kalıbıyla) |
| `build/build.js` | docx üretici (`docx` npm paketi) |
| `diagrams/gen.py` | 9 diyagramın HTML kaynağını üretir (diagram-design stili) |
| `diagrams/export_png.py` | HTML → PNG (Playwright, miniconda Python) |
| `INISIYATIF_KARARLARI.md` | Ayberk'e sorulmadan alınan kararlar; SRS Ek E buradan üretilir |

## Yeniden üretme

```bash
cd docs/srs/diagrams
C:/Users/ayber/miniconda3/python.exe gen.py
C:/Users/ayber/miniconda3/python.exe export_png.py 2.5
cd ../build
npm install
node build.js
```

`node build.js` içindekiler tablosunu boş bırakır; Word'de açıp **Tümünü güncelle (F9)** ile doldurun ya da Word COM ile `TablesOfContents(1).Update()` çalıştırın.
