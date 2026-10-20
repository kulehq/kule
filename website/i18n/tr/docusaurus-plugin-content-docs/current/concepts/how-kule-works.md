---
title: Kule nasıl çalışır
sidebar_label: Kule nasıl çalışır
description: Tek sayfada bütün resim - görevler ve kopyalar, sandbox, proxy, politika, inceleme, commit ve geri alma.
---

# Kule nasıl çalışır

Bu sayfa bir harita. Her parçanın ayrıntıları kendi sayfasında.

## Döngü

```text
  your folder (workspace)
        │  kule new
        ▼
  task copy  ◀── agent or command, inside the SANDBOX
        │          · writes only to the copy and its tmp
        │          · network only through the task's PROXY ──▶ pass / ask you / refuse
        ▼
  effects: what changed in the copy
        │
        ▼
  POLICY verdict for each effect: pass · ask · deny
        │
        ▼
  REVIEW (you) ──▶ commit: apply to the folder, keep a checkpoint ──▶ undo
               └─▶ rollback: throw the changes away
```

1. **Bir klasör üzerinde görev açarsınız.** Kule onun copy-on-write bir
   **kopyasını** oluşturur. → [Görevler ve kopyalar](tasks-and-copies.md)
2. **Bir ajan (ya da herhangi bir komut) kopyada**, işletim sisteminin
   **sandbox**'ı içinde çalışır. Yalnızca kopyaya ve kendi geçici
   dosyalarına yazabilir; kimlik bilgileriniz ve diğer klasörleriniz
   gizlenir. → [Sandbox](sandbox.md)
3. **Ağ trafiği görevin proxy'sinden geçer.** Her bağlantıyı politika
   değerlendirir: paket depoları geçer, ajanın model API'si ajan
   çalışırken geçer, geri kalan her şey sizin yanıtınızı bekler (bir
   **soru**). → [Ağ ve sorular](network-and-questions.md)
4. **Kule etkileri bulur**; bunu kopyayı çalışma klasörüyle karşılaştırarak
   yapar: oluşturulan, değiştirilen, silinen ya da taşınan dosyalar, taşınan
   git dalları, eklenen hook'lar. Komutun ne yaptığını *söylediğine*
   güvenmez; neyin değiştiğine bakar.
5. **Politika her etkiyi değerlendirir**: **pass** (kendiliğinden geçer),
   **ask** (onayınızı bekler) ya da **deny** (reddedilir, asla geçmez). →
   [Politika](policy.md)
6. **Siz incelersiniz**, ardından **commit** edersiniz (klasöre uygulanır,
   bir **kontrol noktası** saklanır) ya da **roll back** yaparsınız
   (değişiklikler atılır). **Undo** bir commit'i geri alır. →
   [İnceleme, commit ve geri alma](review-commit-undo.md)

## Neden izlemek yerine karşılaştırmak?

Kule, bir komutun metnine göre değil, yarattığı **etkilere** göre karar
verir. `rm -rf build`, bir Python betiği ve bir ajanın dosya düzenlemesi
aynı şekilde değerlendirilir: kopyada neyi değiştirdiklerine bakılarak.
Bu yüzden:

- `package.json`'ı yeniden yazan zararsız görünümlü bir komut yine de sizi
  bekler;
- yalnızca kopyaya dokunan korkutucu görünümlü bir komut gerçek hiçbir
  şeyi değiştirmez;
- aynı mantık, sistem çağrılarını izlemeye gerek kalmadan her platformda
  çalışır.

## Ne korunuyor, kimden

Kule, ajanın yanılabileceğini ya da yönlendirilebileceğini varsayar
(örneğin okuduğu bir dosyadaki bir prompt injection ile). Ajanın düzgün
davranmasına güvenmez. Onun yerine sınırlar korur:

| Sınır | Onu ne koruyor |
| --- | --- |
| Ajan ↔ klasörünüz | Kopya, politika ve sizin incelemeniz |
| Ajan ↔ bilgisayarınız | Sandbox (macOS'ta Seatbelt; Linux'ta Landlock, seccomp ve ad alanları) |
| Ajan ↔ ağ | Görevin proxy'si ve politika |
| Ajan ↔ gizli değerleriniz | Yer tutucular; proxy gerçek değerleri yalnızca gizli değerin sunucuları için doldurur |
| Ajan ↔ ev dizininiz | [Ev katmanı](home-layer.md): yazdıkları incelemeyi bekler |

Tam liste için [Güvenlik modeli](../security.md) sayfasına bakın.

## Neyi kim çalıştırır

- **`kule`** komut satırıdır. Motoru doğrudan çalıştırır.
- **Masaüstü uygulaması**, **Kule daemon**'ına açılan bir penceredir.
  Görevler, ajanların çalıştırmaları ve bekledikleri sorular daemon'a
  aittir. Pencereyi kapatmak hiçbir şeyi durdurmaz; daemon on dakika boşta
  kaldıktan sonra kendiliğinden çıkar. Bkz.
  [`kule daemon`](../cli/daemon.md).
- **Ajanlar** (Claude Code, Codex, Gemini ya da Agent Client Protocol
  konuşan herhangi biri) sizin kurduğunuz ayrı programlardır. Kule onları
  sandbox içinde başlatır ve onlarla ACP üzerinden konuşur. Bkz.
  [Ajanlar](../using-agents.md).

## Neler nerede durur

| Nerede | Ne |
| --- | --- |
| Klasörünüz (çalışma klasörü) | Siz commit, sync ya da undo yapana kadar yalnızca okunur. |
| `~/.kule` (Kule'nin ev klasörü ya da `KULE_HOME`) | Görevler, kopyaları, ayarlar, politika, gizli değerler, imzalı geçmiş. Hiçbir sandbox onu göremez. |
| `~/.kule/tasks/<id>/` | Tek bir görev: kopyası, geçici dosyaları, ev katmanı, adımları ve kontrol noktaları. |

Bkz. [Dosyalar ve ayarlar](../reference/files.md).

## Okumaya devam edin

- [Görevler ve kopyalar](tasks-and-copies.md)
- [Sandbox](sandbox.md)
- [Politika](policy.md)
- [Ağ ve sorular](network-and-questions.md)
- [İnceleme, commit ve geri alma](review-commit-undo.md)
- [Eşitleme ve birleştirme](sync-and-merging.md)
- [Git depoları](git.md)
- [Ev katmanı](home-layer.md)
- [Gizli değerler](secrets.md)
- [Bütçeler ve sınırlar](budgets-and-limits.md)
- [Gruplar](groups.md)
