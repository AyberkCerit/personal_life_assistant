# 📘 Proje Başlangıç Dokümanı (PRD)
**Proje Adı:** Personal Life Assistant | **Versiyon:** 1.0.0 

> **Vizyonumuz:** "Kullanıcıların hayatını karmaşık formlar ve menülerle değil, doğal dille ve yapay zekanın analitik gücüyle yönettikleri, sıfır bilişsel yüke sahip bir dijital asistan yaratmak."

---

## 1. Yönetici Özeti (Executive Summary)

**🔴 Problem (Feature Bloat):** Günümüzde zaman, spor ve sağlık yönetimi için kullanılan uygulamalar özellik karmaşası içinde boğulmaktadır. Kullanıcılar bir görevi girmek veya kalori hesaplamak için gereksiz yere çok fazla ekran geçmekte, bu da uzun vadeli kullanım disiplinini bozmaktadır.

**🟢 Çözüm (Minimalizm & AI):** Görev yönetimi (To-Do), fitness planlaması ve sağlık takibini tek bir merkezde toplayan, C++ gücüyle yüksek performanslı ve LLM entegrasyonu sayesinde komutları doğal dille anlayan akıllı bir asistan.

---

## 2. Hedef Kitle (Target Personas)

Uygulamanın mimarisi, hayatını verilerle yönetmek isteyen spesifik personalara göre optimize edilmiştir:
* 👔 **Yoğun Profesyoneller:** Zaman yönetimini NLP (Doğal Dil İşleme) ile hızlandırmak isteyenler.
* 🏋️ **Disiplinli Sporcular:** Push-Pull-Legs (PPL) gibi spesifik rutinleri ve makro dengesini veri odaklı takip edenler.
* 🎓 **Üretken Öğrenciler:** Günlük akademik hedeflerini sağlık verileriyle dengelemek isteyenler.

---

## 3. Minimum Viable Product (MVP) Özellikleri

Uygulamanın ilk canlı sürümü (MVP) aşağıdaki 4 çekirdek modül üzerine inşa edilecektir:

| Modül | Temel İşlev (Core Function) | Entegrasyon Durumu |
| :--- | :--- | :--- |
| **📝 To-Do List** | Hızlı görev ekleme, silme ve tamamlama | Sistemin ana iskeleti |
| **💪 Gym Plan** | Antrenman setleri, ağırlık ve idman süreleri takibi | To-Do'ya görev olarak atanır |
| **❤️ Health** | Adım, uyku süresi ve kalori açığı/fazlası takibi | Gym ile kalori verisi paylaşır |
| **🧠 AI Asistan** | NLP ile veri girişi ve haftalık/günlük özet analizleri | Tüm modülleri okur ve yorumlar |

---

## 4. Teknik & Mimari Gereksinimler (NFR)

* **⚡ Performans:** Sistem, düşük gecikmeli (low-latency) veritabanı sorguları ve C++ bellek yönetimi optimizasyonları ile çalışacaktır.
* **🔒 Gizlilik (Privacy-First):** Kişisel sağlık, kilo ve günlük rutin verileri işlendiğinden, veriler dış sunuculara gönderilmeden (AI API çağrıları hariç) yerel veritabanında (Local DB) tutulacaktır.
* **🔄 Ölçeklenebilirlik (Scalability):** Proje, modüler (Agile) bir yapıda tasarlandığı için ilerleyen süreçte yeni modüllerin (Örn: Finans Takibi) eklenmesine açıktır.

---

## 5. Başarı Kriterleri (Success Metrics / KPIs)
*Bu projenin başarılı sayılması için MVP sonrası şu metrikler takip edilecektir:*
1. **Sisteme Bağlılık (Retention):** AI asistanı sayesinde kullanıcıların uygulamada geçirdiği "gereksiz form doldurma" süresinin %50 azaltılması.
2. **Kusursuz Entegrasyon:** Gym modülüne girilen efor verisinin, anlık olarak Health modülündeki kalori dengesine "0" hata payı ile yansıması.

---

## 6. Gelecek Vizyonu (V2.0 Scope)
*MVP tamamlandıktan sonra planlanan geliştirme havuzu:*
* Giyilebilir teknolojilerle (Smartwatches) adım/kalori API entegrasyonu.
* AI asistanın sesli komutları (Voice-to-Text) desteklemesi.