---
title: kule status
sidebar_label: kule status
description: Bir görevin commit edilmemiş değişikliklerini, fonksiyon ve bölüm düzeyine kadar, ve politikanın bunlar hakkındaki kararını gösterir.
---

# kule status

Görevin commit edilmemiş değişikliklerini ve politikanın bunlar hakkında ne
dediğini gösterir.

## Kullanım

```sh
kule status [OPTIONS] <TASK>
```

## Açıklama

`kule status`, görevin kopyasını çalışma klasörüyle karşılaştırır ve commit
etmeden önce size şunları söyler:

- **Ne değişti:** oluşturulan, değiştirilen, silinen ve taşınan dosyalar;
  her dosyanın içinde de neyin değiştiği, bir insanın kullandığı adlarla:
  Rust, C#, TypeScript, JavaScript, Python ve Go'da fonksiyonlar, metotlar,
  alanlar ve tipler; Markdown'da bölümler; JSON ve TOML'da anahtarlar.
- **Git tarafında ne oldu** (klasör bir depoysa): dal hareketleri, yeni
  nesneler, index, config ve hook değişiklikleri. Bkz.
  [Git](../concepts/git.md).
- **Politika ne diyor:** değişiklikler kendiliğinden mi geçiyor, onayınızı
  mı bekliyor, yoksa reddediliyor mu; her birine hangi kuralın karar
  verdiği. Bkz. [Politika](../concepts/policy.md).
- **Ev dizininize yapılan ve incelemeyi bekleyen yazmalar** (bkz.
  [kule home](home.md)).
- **Başka görevlerin bu görevle kesiştiği yerler:** aynı klasör üzerindeki
  ve aynı dosyaları, fonksiyon ya da bölüm düzeyine kadar değiştiren
  görevler.
- **Çalışma klasörünün bu arada neyi değiştirdiği:** kopya son güncellendiğinden
  beri gerçek klasörde (sizin ya da başka bir görevin commit'iyle) değişen
  dosyalar; bunları [kule sync](sync.md) kopyaya alabilir.
- **Görevin ajan çalıştırmalarının ne harcadığı** ve bütçesinden ne kaldığı.

Etkiler komutlar izlenerek değil, kopya çalışma klasörüyle karşılaştırılarak
bulunur. Bu yüzden `status` her şeyi gösterir: ajanın işini, `kule shell`
içinde kendi yaptığınız düzenlemeleri ve bir `kule exec` komutunun yaptığı
her şeyi.

Yalnızca okur; hiçbir şeyi değiştirmez.

## Argümanlar

| Ad | Zorunlu | Anlamı |
| --- | --- | --- |
| `TASK` | evet | Görevin tam kimliği. Bkz. [Bir göreve ad vermek](index.md). |

## Seçenekler

| Seçenek | Değer | Varsayılan | Anlamı |
| --- | --- | --- | --- |
| `--json` | | | Metin yerine tek bir JSON nesnesi yazdırır. Bkz. [JSON çıktısı](#json-output). |
| `--home` | yol | `~/.kule` | Kule'nin ev klasörü (ya da `KULE_HOME`). Bkz. [genel seçenekler](index.md). |

## Örnekler

**Kendiliğinden geçen değişiklikler:**

```text
$ kule status 3efe1ca8
2 file changes: +1 ~1 -0
  + NOTES.md
  ~ src/main.rs
      ~ function main
      + function wave
policy: passes on its own
```

**Sizi bekleyen bir değişiklik** (yerleşik *balanced* politikasında bir
manifest dosyası):

```text
$ kule status 3efe1ca8
3 file changes: +1 ~2 -0
  ~ Cargo.toml
      + key dependencies
  + NOTES.md
  ~ src/main.rs
      ~ function main
      + function wave
policy: waits for approval
  decides what gets installed (manifests-and-lockfiles) · waits for approval
    Cargo.toml
```

**Silmelerden ayrı gösterilen taşımalar:**

```text
$ kule status 3422f830
6 file changes: +1 ~0 -2 »3
  + docs/
  » report.pdf → docs/report.pdf
  - fake.pdf
  » src/ → lib/
  - run.sh
policy: passes on its own
```

**Git ve reddedilen bir değişiklik:**

```text
$ kule status 3422f830
git: hook pre-commit changed (it runs on the next matching git command)
policy: denied
  a git hook, which runs on its own (git-hooks) · denied
    git hook pre-commit
```

**Klasör bu arada değişti:**

```text
$ kule status 3efe1ca8
1 file changes: +0 ~1 -0
  ~ src/main.rs (changed in the workspace too; not outlined)
policy: passes on its own
the workspace changed since the task's copy (kule sync 3efe1ca8 brings it in):
  README.md
  src/main.rs (the task changed it too)
```

**Aynı klasördeki iki görev kesişiyor:**

```text
$ kule status 3422f830
1 file changes: +0 ~1 -0
  ~ README.md
      ~ section App
policy: passes on its own
task 3efe1ca8 changes them too:
  README.md · section App
```

**Yok sayılan yollar geride kalır:**

```text
$ kule status 3422f830
2 ignored by the workspace, staying in the copy (commit --include-ignored takes them):
  · target/
  · target/app
1 file changes: +0 ~1 -0
  ~ README.md
      ~ section App
policy: passes on its own
```

## Çıktı

| Satır | Anlamı |
| --- | --- |
| `no changes` | Kopya, çalışma klasöründekinin aynısını tutuyor. |
| `N file changes: +a ~m -d »v` | Sayılar: oluşturulan, değiştirilen, silinen, taşınan. |
| `+ path`, `~ path`, `- path` | Oluşturulan, değiştirilen ya da silinen bir dosya veya klasör (`/` ile biter). |
| `» from → to` | Bir taşıma ya da yeniden adlandırma. Klasörüyle birlikte taşınan bir dosya ayrıca listelenmez. |
| girintili `+`/`~`/`-` satırları | Üstteki dosyanın içinde neyin değiştiği: `function main`, `method Task::commit`, `section Plan › Later`, `key dependencies`. Dosyadaki her değişiklik bunlardan birinin ya da dosyanın üst düzeyinin altında listelenir. |
| `(… not outlined)` | Kule bu dosyanın içine bakamadı ve nedenini söylüyor (örneğin çalışma klasörü de onu değiştirdi). |
| `N ignored by the workspace, …` | Çalışma klasörünün `.gitignore`'unun dışarıda bıraktığı yollar. Düz bir commit bunları kopyada bırakır. |
| `git: main a141d88 → 4e385b1 · fast-forward · 1 commit` | Dal ilerledi. `fast-forward` yerine `history rewritten` görünüyorsa commit'ler değiştirilmiş demektir. |
| `git: 3 new object files · index changed` | Yeni git nesneleri (yalnızca eklenir) ve güncellenmiş bir index. |
| `git: config changed (…)`, `git: hook … changed (…)` | Bir sonraki git komutunun kod çalıştırmasına yol açan değişiklikler. |
| `git: <path> is staged unlike the file (…)` | Index bu dosya için dosyanın kendisinden farklı bir içerik tutuyor; yani bir sonraki `git commit` kimsenin incelemediği bir şeyi kaydeder. Sizi bekler. |
| `in your home, caught for review` | Ev dizininize yapılan yazmalar; bkz. [kule home](home.md). |
| `policy: <verdict>` | Genel karar: `passes on its own` (kendiliğinden geçer), `waits for approval` (onay bekler) ya da `denied` (reddedilir). |
| `  <reason> (<rules>) · <verdict>` | Doğrudan geçmeyenler, nedenlerine göre bir araya toplanmış olarak: sade sözcüklerle neden, arkasındaki kurallar ve karar. Yalnızca türü yüzünden bekleyen dosyalar, her tür için bir grup olur (`a kind of file this project has none of: weird`). Bkz. [Politika kuralları](../reference/policy-rules.md). |
| `    <subject>` | Üstteki gruptaki bir değişiklik: bir yol ya da `git hook pre-commit` gibi bir git değişikliği. |
| `digest: <hex> (to approve these: …)` | Bu değişikliklerin hash'i. `kule commit TASK --approve --reviewed <hex>` onları yalnızca hâlâ bunlar oldukları sürece commit eder. |
| `home digest: <hex>` | Ev dizininizdeki yazmalar için aynısı; bkz. [kule home](home.md). |
| `task <id> changes them too:` | Aynı klasördeki başka bir görev aynı dosyaları değiştiriyor; altındaki satırlar dosyayı ve fonksiyonu, bölümü ya da anahtarı adlandırır. |
| `the workspace changed since the task's copy …` | `kule sync`'in kopyaya alacakları. `(the task changed it too)`, iki tarafın da değiştirdiği dosyaları işaretler. |
| `workspace <path> is gone (moved or deleted); commit and undo are refused` | Gerçek klasör artık yerinde değil. |
| `spent: …; budget …, … left` | Görevin ajan çalıştırmalarının şimdiye kadarki maliyeti. Bir çalıştırma olduktan ya da bir bütçe belirlendikten sonra gösterilir. Çalıştırmaların harcadıklarına dair bazı kayıtlar okunamıyorsa ` (N records cannot be read)`; onları sayan bir bütçe bir sonraki çalıştırmayı tutar (bkz. [kule budget](budget.md)). Çalıştırmalar bir pay tutuyorsa ` (<amount> set aside for runs at work or that did not end)`; bu kalandan düşülür. |

En fazla 40 dosya listelenir; geri kalanlar sayılır (`… N more (kule status --json)`).

## JSON çıktısı {#json-output}

`kule status TASK --json` tek bir nesne yazdırır. Alanları:

| Alan | İçeriği |
| --- | --- |
| `effects` | `effects`: değişen her yol; `path`, `change` (`created`, `modified`, `deleted`) ve `kind` (`file`, `dir`, …) ile. Varsa taşımalar, git değişiklikleri ve yok sayılan yollar, ayrıca `repository` (klasörün bir git deposu olup olmadığı). |
| `ruling` | `verdict` (`allow`, `ask` ya da `deny`) ve `judgements`: her konu için bir tane; `subject`, `verdict` ve karar veren `rules` ile. |
| `outline` | Değişen her dosya için içinde neyin değiştiği, ya da bir `reason` ile birlikte `"outline": "skipped"`. |
| `spent` | `runs`, `tokens`, `cost` (tutar ve para birimlerinden oluşan bir liste) ve `unpriced_runs`; okunamayan kayıt varsa `unreadable` (okunamayan kayıtların sayısı). Çalıştırmalar bir pay tutarken: `held` (ayırdıkları, para birimi başına bir tutar; her çalıştırmanın kullanımı gelene kadar harcanmış sayılır) ve `held_by` (o çalıştırmaların bulunduğu görevler). |
| `budget` | Görevin kendi bütçesi ya da `null`. |
| `home` | Ev dizininize yapılan ve incelemeyi bekleyen yazmalar. |
| `behind` | `changed`: son sync'ten beri çalışma klasörünün değiştirdiği yollar; `both`: görevin de değiştirdikleri; `repository`: commit'lerin, dalların ya da index'in değişip değişmediği. |
| `digest` | Düz bir commit'in alacağı değişikliklerin hash'i, `kule commit --approve --reviewed` için; değişiklik yoksa `null`. |

```json
{
  "effects": {
    "effects": [
      { "path": "src/main.rs", "change": "modified", "kind": "file" }
    ],
    "repository": true
  },
  "ruling": {
    "verdict": "allow",
    "judgements": [
      { "subject": "src/main.rs", "verdict": "allow", "rules": ["files"] }
    ]
  },
  "outline": [
    { "path": "src/main.rs", "outline": "skipped", "reason": "before_unknown" }
  ],
  "spent": { "runs": 0, "tokens": 0, "cost": [], "unpriced_runs": 0 },
  "budget": null,
  "home": [],
  "behind": { "changed": ["src/main.rs"], "both": ["src/main.rs"], "repository": false },
  "digest": "9f2c…e41a"
}
```

Yalnızca kendiliğinden geçenleri commit eden bir betik:

```sh
if [ "$(kule status "$TASK" --json | jq -r .ruling.verdict)" = allow ]; then
  kule commit "$TASK"
fi
```

(`kule commit` zaten kendi başına da `3` çıkış koduyla reddeder.)

## Uygulamada

**Review** paneli (Mod+2) aynısını Commit ve Roll back düğmeleriyle
gösterir. Bkz. [Review](../app/review.md).

## Ayrıca bakın

- [İnceleme, commit ve geri alma](../concepts/review-commit-undo.md)
- [kule commit](commit.md), [kule rollback](rollback.md), [kule sync](sync.md), [kule steps](steps.md)
