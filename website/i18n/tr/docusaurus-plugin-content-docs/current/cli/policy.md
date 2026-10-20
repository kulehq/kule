---
title: kule policy
sidebar_label: kule policy
description: Tüm görevler için geçerli politikayı yazdırın ya da Kule'nin balanced ve careful politikaları arasında geçiş yapın.
---

# kule policy

Tüm görevler için geçerli politikayı yazdırır ya da Kule'nin iki yerleşik
politikası arasında geçiş yapar.

## Kullanım {#synopsis}

```sh
kule policy [OPTIONS] [CHOICE]
```

## Açıklama {#description}

**Politika**, bir görevin yaptığı her şeyi klasörünüze ulaşmadan önce
değerlendiren bir [Cedar](https://www.cedarpolicy.com/) kuralları kümesidir:
her dosya değişikliğini, her git değişikliğini ve her ağ bağlantısını. Her
değerlendirme üç karardan biridir:

- **kendiliğinden geçer**: `kule commit` sormadan uygular;
- **onay bekler**: `kule commit --approve` ya da uygulamada *Approve and
  commit* gerekir. Onay bekleyecek bir bağlantı uygulamada size sorulur,
  komut satırından ise hemen reddedilir;
- **reddedilir**: onayla bile asla geçmez.

Kule'nin kendine ait iki politikası vardır:

| Politika | Kendiliğinden geçenler | Onay bekleyenler |
| --- | --- | --- |
| `balanced` (varsayılan) | Sıradan dosya değişiklikleri, ileri sarma (fast-forward) dal hareketleri ve yeni dallar, HTTPS üzerinden paket kayıt depoları. | Manifest ve kilit dosyaları, CI ve editör ayarları, kod çalıştıran ya da bir ajanı yönlendiren dosyalar, bağlantılar (link), iç içe depolar, git config, geçmişi yeniden yazma, toplu silmeler. Git hook'ları reddedilir. |
| `careful` | Yalnızca `balanced`'ın geçirdiği bağlantılar (paket kayıt depoları ve her ajanın çalışırken kendi host'ları). | Bir dosyaya ya da git'e yapılan her değişiklik. |

Seçim vermeden `kule policy`, tüm görevler için geçerli kuralları yazdırır.
`kule policy balanced` ve `kule policy careful` bunlardan birini devreye
alır. Kendi politikası olan bir [gruptaki](group.md) görev ise grubun
politikasına göre değerlendirilir.

### Kendi politikanız {#your-own-policy}

Kendi politikanızı yazmak için onu Kule'nin evine `policy.cedar` olarak
kaydedin (`~/.kule/policy.cedar`). Bu dosya tüm görevler için Kule'nin
politikasının **yerine geçer** ve Kule onun üzerine asla yazmaz: dosya
oradayken `kule policy balanced` ve `careful` reddedilir. Geri dönmek için
dosyayı başka yere taşıyın. Her yerleşik kural ve adım adım bir örnek için
[Politika kuralları](../reference/policy-rules.md) sayfasına bakın.

### Bir deponun kuralları {#a-repositorys-rules}

Bir depo, kökündeki `.kule/policy.cedar` dosyasına **`forbid` kuralları**
ekleyebilir. Bunlar politikayı yalnızca sıkılaştırır, asla gevşetmez ve
bir ajanın düzenlediği kopyadan değil, her zaman gerçek klasörünüzden
okunur.

## Argümanlar {#arguments}

| Argüman | Anlamı |
| --- | --- |
| `CHOICE` | `balanced` ya da `careful`. Geçerli politikayı yazdırmak için boş bırakın. |

## Seçenekler {#options}

| Seçenek | Değer | Varsayılan | Anlamı |
| --- | --- | --- | --- |
| `--home` | yol | `~/.kule` (ya da `KULE_HOME`) | Kule'nin evi. Bkz. [genel seçenekler](index.md). |
| `-h`, `--help` | | | Yardımı yazdırır. |

## Örnekler {#examples}

Geçerli politikayı yazdırın (balanced politikanın ilk satırları):

```console
$ kule policy | head -3
// Kule's built-in policy. Each effect is asked twice: may it pass on its own
// (Action "auto"), and if not, may a person approve it (Action "ask").
// A forbid always wins over a permit.
```

Her değişikliğin sizi beklemesini sağlayın:

```console
$ kule policy careful
policy: careful
```

Varsayılana geri dönün:

```console
$ kule policy balanced
policy: balanced
```

Bir görevin değişikliklerinin politikadan ne karar aldığını görün:

```console
$ kule status 1046fb30
1 file changes: +0 ~1 -0
  ~ Cargo.toml
policy: waits for approval
  Cargo.toml · waits for approval (manifests-and-lockfiles)
```

Parantez içindeki sözcük kuralın `@id`'sidir; [Politika kuralları](../reference/policy-rules.md)
her birini açıklar.

## Çıktı {#output}

- `kule policy`, Cedar metninin kendisini yazdırır: Kule'nin balanced
  politikası, Kule'nin careful politikası (sonuna bir `careful` kuralı
  eklenmiş balanced metni) ya da kendi dosyanız.
- `policy: careful` / `policy: balanced` bir geçişi onaylar.

## Hatalar ve çözümler {#errors-and-fixes}

| Mesaj | Ne yapmalı |
| --- | --- |
| `kule: strict: the policies to choose are balanced and careful` | İkisinden birini kullanın. Başka bir şey için kendi `policy.cedar` dosyanızı yazın. |
| `kule: ~/.kule/policy.cedar is a policy of your own; Kule does not replace it` | Kule'nin politikasını geri istiyorsanız önce kendi dosyanızı taşıyın ya da silin. |
| `exec`, `status`, `commit` ya da `run`'dan `kule: policy: unexpected end of input` (ya da başka bir Cedar hatası) | `policy.cedar` dosyanız, bir grubun politikası ya da deponun `.kule/policy.cedar` dosyası ayrıştırılamıyor. Kule bozuk bir politikayla karar vermeyi reddeder; bu yüzden düzeltilene kadar hiçbir şey çalışmaz ve commit edilmez. |
| `kule: .kule/policy.cedar may only contain forbid rules: a repository can tighten the policy, never loosen it` | Deponun dosyasındaki `permit` kurallarını kaldırın. |

## Uygulamada {#in-the-app}

*Settings → Policy*, tüm görevler için balanced ya da careful politikayı ve
her grup için bir politikayı seçer. İlk açılış ekranı da aynı seçimi sorar.

## Ayrıca bakın {#see-also}

- [Politika](../concepts/policy.md): kararlar nasıl işler.
- [Politika kuralları](../reference/policy-rules.md): tüm yerleşik kurallar.
- [`kule rules`](rules.md): Kule'ye bir daha sormamasını söyledikleriniz.
- [`kule group`](group.md): bir grubun politikası.
- [`kule commit`](commit.md): `--approve` ve `--remember`.
