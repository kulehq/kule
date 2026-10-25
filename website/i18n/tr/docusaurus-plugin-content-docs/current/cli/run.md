---
title: kule run
sidebar_label: kule run
description: Bir yapay zekâ ajanına bir prompt verir; ajan görevin kopyasında, sandbox içinde çalışır ve her araç çağrısı bir adım olur.
---

# kule run

Bir ajana bir istem (prompt) verir. Ajan görevin kopyasında, sandbox
(yalıtılmış ortam) içinde, ağ trafiği görevin proxy'sinden geçerek çalışır.
Siz commit edene kadar çalışma klasörüne hiçbir şey ulaşmaz.

## Kullanım

```sh
kule run [OPTIONS] <TASK> <PROMPT>
```

## Açıklama

`kule run`, Claude Code, Codex ya da Gemini gibi bir ajanı başlatır, ona
prompt'unuzu verir ve sırası bitene kadar yaptıklarını yazdırır. Kule
ajanla, bir editörün yapacağı gibi **Agent Client Protocol (ACP)**
üzerinden konuşur.

Ajanın yapabilecekleri:

- **Dosyalar:** görevin kopyasını okur ve yazar. Gerçek klasörünüze
  yazamaz; kimlik bilgilerinizi (`~/.ssh`, `~/.aws`, …), kendi
  klasörlerinizi (Documents, Desktop, …) ya da diğer ajanların oturum
  bilgilerini okuyamaz. Bkz. [Sandbox](../concepts/sandbox.md).
- **Ağ:** her bağlantı görevin proxy'sinden geçer. Paket depoları ve ajanın
  kendi model API'si geçer; geri kalan her şeye
  [politika](../concepts/policy.md) karar verir. CLI'da soracak kimse
  olmadığından, politikanın bir insana soracağı bir bağlantı **hemen
  reddedilir**. (Uygulamada sizin cevabınızı bekler.) Bkz.
  [Ağ ve sorular](../concepts/network-and-questions.md).
- **Ev dizininiz:** `~/.zshrc`, `~/.config` ve benzerlerine yazdıkları,
  [kule home](home.md) ile incelemeniz için görevin ev katmanında (home
  layer) yakalanır; kendiliğinden asla ev dizininize yazılmaz.
- **Gizli değerler:** yer tutucuları görür, API anahtarlarınızı asla görmez.
  Bkz. [Gizli değerler](../concepts/secrets.md).

Ajanın dosya değiştiren her araç çağrısı bir **adım** olur (bkz.
[kule steps](steps.md)). Çalıştırma bittiğinde Kule kopyada neyin
değiştiğini ve politikanın bunun hakkında ne dediğini yazdırır; böylece
doğrudan [kule commit](commit.md) ya da [kule rollback](rollback.md)
komutuna geçebilirsiniz. Önce ajanı yeniden de çalıştırabilirsiniz:
siz karar verene kadar değişiklikler kopyada birikir.

**Bütçeler ve sınırlar geçerlidir.** Görevin bütçelerinden biri (kendi
bütçesi, grubununki ya da tüm görevler için olan günlük bütçe) tükendiğinde
ya da görev süresini doldurduğunda çalıştırma başlamaz. Çalışırken
işlemci, bellek, disk, süre ve boşta kalma
[sınırları](../concepts/budgets-and-limits.md), uygulamada olduğu gibi onu
tutar. Sınırlar, çalıştırmanın başlattığı her süreci sayar; ajanın süreç
ağacından ayrılmış olanı da.

**Ajan, Kule'nin tutabileceği kadarıyla sınırlıdır.** Ajanın 16 MiB'tan uzun
bir mesajı ya da aynı anda açık bıraktığı 1024'ten fazla araç çağrısı
çalıştırmayı durdurur. Bir araç çağrısının başlığı 200 karaktere kısaltılır.
Kule ajanın komutlarını onun yerine çalıştırdığında (uygulamada), aynı anda
en çok 32 komut çalıştırır.

**Başlattığı hiçbir şey çalıştırmadan uzun yaşamaz.** Çalıştırma nasıl
biterse bitsin, ajanın sandbox'ında çalışır bıraktığı her şey de biter;
ebeveynine ne olmuş olursa olsun. Kule neyi sonlandırdığını bir notta adıyla
söyler:
`· Kule ended <names>, which the agent left running in its sandbox, as the run ended`.

**Başarısız olan ya da durdurulan bir çalıştırma da eksiksiz anlatılır.**
Kule, iyi giden bir çalıştırmadan sonra olduğu gibi neyin değiştiğini,
bağlantılarını, ev dizinine yazdıklarını, sandbox'ın reddettiklerini,
Kule'nin geri koyduğu yolları, politikanın kararını, ne harcandığını ve ne
kadar sürdüğünü yazdırır; hatayı ancak ondan sonra yazar. Linux'ta başarısız
olan bir çalıştırma ayrıca
`kule: Linux does not tell Kule what the sandbox refused; each refusal reached the command as an error`
yazdırır. Kule çalıştırmanın harcadığını kaydedemediyse, değiştirdiğini geri
koyamadıysa ya da sandbox'ın reddettiklerini okuyamadıysa veya
kaydedemediyse, her biri için bir `kule:` satırıyla bunu söyler ve
`kule run` başarısız olur.

**Ajan neyin değiştiğini duyar.** Ajanın son çalıştırmasından bu yana
[kule sync](sync.md) klasördeki değişiklikleri kopyaya aldıysa ya da
sandbox ona Kule'nin açıklayabildiği bir şeyi reddettiyse, prompt'unuzdan
önce Kule'nin kendi sözleriyle kısa bir not bir kez gider.

Bir görevde aynı anda tek bir ajan çalışır. Ajanları yan yana çalıştırmak
için daha fazla görev açın (aynı klasörde ya da başkalarında).

:::note

Ajanın, Kule olmadan nasıl kuruluyor ve oturum açıyorsa öyle kurulmuş ve
oturum açmış olması gerekir: Claude Code ve Codex Node.js (`npx`), Gemini
kendi CLI'ını ister. `kule agents`, burada hangi ajanların başlayabildiğini
ve diğerlerinin neden başlayamadığını listeler. Bkz. [Ajanlar](../using-agents.md).

:::

## Argümanlar

| Ad | Zorunlu | Anlamı |
| --- | --- | --- |
| `TASK` | evet | Görevin tam kimliği. Bkz. [Bir göreve atıf yapmak](index.md). |
| `PROMPT` | evet | Ajanın ne yapması gerektiği. Tırnak içine alın: tek bir argüman olmalı. |

## Seçenekler

| Seçenek | Değer | Varsayılan | Anlamı |
| --- | --- | --- | --- |
| `--agent` | ad | `claude-code` | Prompt'u hangi ajanın alacağı: `claude-code`, `codex`, `gemini` ya da eklediğiniz ve onayladığınız bir ajan profilinin adı (bkz. [kule agents](kule-agents.md)). |
| `--home` | yol | `~/.kule` | Kule'nin ev klasörü (`KULE_HOME` ile de verilebilir). Bkz. [genel seçenekler](index.md). |

## Örnekler

**Claude Code'dan bir testi düzeltmesini istemek:**

```sh
kule run 3422f830 "The test in tests/greet.rs fails. Fix it and run cargo test."
```

**Onun yerine Codex kullanmak:**

```sh
kule run 3422f830 --agent codex "Add a --verbose flag to the CLI."
```

**Kod yerine belgeler üzerinde çalışmak:**

```sh
kule run 40ac70b8 "Sort the notes in this folder into one folder per year. Keep the file names."
```

**Çalıştırmak, kararı kontrol etmek ve yalnızca her şey kendiliğinden geçiyorsa commit etmek:**

```sh
kule run "$TASK" "Update the README's install section" && kule commit "$TASK"
```

Onayınızı bekleyen herhangi bir şey varsa `kule commit` `3` ile çıkar ve
hiçbir şey commit etmez; bu yüzden bu güvenlidir.

## Çıktı

Ajan çalışırken bir meşgul satırı ne yaptığını söyler. Bir çalıştırmanın
çıktısı şuna benzer (ajanın sözleri ve araçları değişir):

```text
I'll look at the failing test first.

  ✓ Read tests/greet.rs → step 1: no file changes
  ✓ Edit src/main.rs → step 2: src/main.rs
  ✓ cargo test → step 3: target/debug/app and 41 more
plan: Read the test / Fix greet / Run the tests

The greeting now matches what the test expects, and cargo test passes.

1 file changes: +0 ~1 -0
  ~ src/main.rs
      ~ function greet
net: index.crates.io:443 · tunneled (package-registries)
policy: passes on its own
spent: 0.12 USD, 18234 tokens in 1 run
took: 41s
```

| Satır | Anlamı |
| --- | --- |
| düz metin | Ajanın cevabı, yazdığı gibi. |
| `✓ <tool> → step N: <files>` | Başarılı bir araç çağrısı, dönüştüğü adım ve değiştirdiği dosyalar (ilk beşi; geri kalanı `kule steps`'te). `✗` başarısız olduğu anlamına gelir. |
| `plan:` | Ajan paylaşırsa planı. |
| `· <note>` | Kule'den ya da ajandan bir not. |
| `· Kule allowed "<call>" (the agent's option: "<label>")` | Ajan izin istedi ve Kule ne yaptı; bu, seçtiği seçeneğin türünden gelir, ajanın kendi etiketi yalnızca arkasından tırnak içinde gelir. Seçenek reddediyorsa `Kule did not allow …` (bir terminalde yalnızca bunlar `✗` ile görünür). |
| `N file changes: +a ~m -d` | Kopyada olup çalışma klasöründe olmayanlar: oluşturulan, değiştirilen, silinen. Her dosyanın içinde değişen fonksiyonlar, bölümler ya da anahtarlar. Bkz. [kule status](status.md). |
| `git:` | Depoya ne olduğu: dal hareketleri, yeni nesneler, config ya da hook değişiklikleri. Bkz. [Git](../concepts/git.md). |
| `net:` | Çalıştırmanın yaptığı her bağlantı ve ona ne olduğu: geçtiyse `tunneled`; Kule onu kestiyse ya da içindeki bir isteği reddettiyse `cut`; kendi hatasıyla bittiyse (host gitti, bağlantı sıfırlandı) `failed`; politika geçirmediyse `refused, …`. `cut` ve `failed` için nedeni `network.jsonl` söyler. |
| `in your home, caught for review` | Ev klasörünüze yapılan ve ev katmanında bekleyen yazmalar. Bkz. [kule home](home.md). |
| `sandbox: refused …` | Sandbox'ın reddettikleri (macOS bunları listeler; Linux'ta komut yalnızca bir hata alır). |
| `sandbox: macOS's log dropped some kernel messages …` | Gri, macOS'te: sistem günlüğünün çalıştırma sırasında çekirdeğin mesajlarını ne zaman düşürdüğü; yoğun bir Mac bunu birkaç saniyede bir yapar. O anda yapılmış bir ret listede yoktur. Bkz. [kule exec](exec.md#output). |
| `sandbox: <path> changed during the run; Kule put it back …` | Çalıştırmanın değiştiremeyeceği, Kule'nin eski hâline geri koyduğu bir yol. |
| `policy:` | Değişiklikler hakkındaki karar: *passes on its own* (kendiliğinden geçer), *waits for approval* (onay bekler) ya da *denied* (reddedildi), her birinin arkasındaki kuralla birlikte. |
| `spent:` | Görevin çalıştırmalarının, ajanların bildirdiğine göre, şimdiye kadar ne kadar tuttuğu ve bütçesinden ne kaldığı. Görevin kullanım kayıtlarından bazıları okunamıyorsa ` (N records cannot be read)`. Çalıştırmalar, kullanımlarının henüz kapatmadığı bir pay tutuyorsa ` (<amount> set aside for runs at work or that did not end)`; bu kalandan düşülür. |
| `took:` | Çalıştırmanın ne kadar sürdüğü. |

Çıktı bir terminal değilse her olay kendi düz satırında yazılır; bu,
günlüğe kaydetmeyi kolaylaştırır.

## Hatalar ve çözümleri

| Mesaj | Ne yapmalı |
| --- | --- |
| `kule: no agent <name>; known: claude-code, gemini, codex` | Adı kontrol edin. Kendi ajanlarınızı [kule agents](kule-agents.md) ile ekleyin. |
| `kule: agent <name> cannot run: <why>` | Ajan eksik, oturum açılmamış ya da (kendi profilleriniz için) henüz onaylanmamış. Nedenini görmek için `kule agents` çalıştırın. |
| `kule: task <id> has spent its budget; raise it or turn it off to run again` | `kule budget TASK <amount>` ile artırın ya da `kule budget TASK off` ile kapatın. Tükenen bir grubun ya da günlük bütçeyse mesaj onun adını verir. |
| `kule: Kule cannot tell what <budget> has left: <N> record(s) of what runs spent … cannot be read; mend or remove them, or turn the budget off, to run again` | Okunamayan bir kullanım kaydı da sayılır; bu yüzden bütçe, kayıt düzeltilene ya da kaldırılana (bir görevin `usage.jsonl` dosyası) veya bütçe kapatılana kadar her çalıştırmayı tutar. Bkz. [Bütçeler ve sınırlar](../concepts/budgets-and-limits.md). |
| ``kule: what is left of <budget> (<amount>) is set aside for runs at work, or that did not end, in task <id>: once a run ends, what it spent counts instead; `kule budget TASK --release` lets go of one that did not end`` | Ondan önce başlayan çalıştırmalar (adı verilen görevlerde) kalanı tutuyor. Bitmelerini bekleyin. Bitmeyen bir çalıştırmanın (Kule durdurulmuştu) payı, siz onu [kule budget](budget.md) `TASK --release` ile bırakana kadar sayılmaya devam eder. |
| `kule: Kule could not set aside what the run may spend (…), so it does not start` | Kule görevin `reserved.jsonl` dosyasına yazamadı. Hiçbir şey başlamadı. Kule'nin ev klasörüne yazılabildiğini kontrol edin. |
| `kule: the task has used its time: its runs took N minutes together; …` | Görevin süre sınırını [kule limits](limits.md) ile artırın ya da kaldırın. |
| `kule: an agent is at work in task <id> …` | Görev başına tek ajan. Bekleyin ya da başka bir görev açın. |
| `kule: a terminal is open on this task's copy (in Kule's app, or kule shell); close it first, then start work` | Görevde bir terminal (uygulamanınki ya da `kule shell`) açık. Önce onu kapatın. |
| `kule: Kule opened task <id>'s copy in <app> outside the sandbox; …` | O terminal uygulamasını ya da editörü kapatın, sonra [kule closed](closed.md) `TASK` çalıştırın. |
| `kule: <error> (see <home>/tasks/<id>/agent.log)` | Ajan başarısız oldu. Bu satır, çalıştırmanın yaptıklarından sonra gelir. Ajanın kendi mesajları `agent.log` içinde: son 8 MB'ı. Günlük çalıştırma sırasında yenilendiyse (eskisi `agent.log.1` olduysa) satır `, and agent.log.1 beside it for what came before` ile biter: ondan önceki 8 MB. Yenilenmiş bir günlük `--- Kule turned this log over here …` satırıyla başlar; bu satır daha eski bir `agent.log.1`'in bırakıldığını da söyler. |
| `kule: stopped: <why>` | Bir sınır ya da ajanın kendi mesajları çalıştırmayı durdurdu; satırın gerisi nedenini söyler. Bir günlük adı vermez. |
| `kule: stopped: the agent sent a message larger than 16 MiB` | Ajan tek mesajda Kule'nin aldığından fazlasını söyledi. Daha dar bir istemle yeniden çalıştırın. |
| `kule: stopped: the agent left more than 1024 tool calls open` | Ajan araç çağrılarını bitirmeden yenilerini başlattı. |
| `kule: Linux does not tell Kule what the sandbox refused; each refusal reached the command as an error` | Linux'ta, başarısız olan bir çalıştırmadan sonra: hiç `sandbox: refused` satırı olmaması hiçbir şeyin reddedilmediği anlamına gelmez: reddedilen bir işlem ajanın komutuna bir izin hatası olarak ulaştı. |
| `kule: what the run spent could not be recorded (…), so the <amount> set aside for it stays counted` | Kule görevin `usage.jsonl` dosyasına yazamadı. Harcadığının yerine, siz [kule budget](budget.md) `TASK --release` ile bırakana kadar çalıştırmanın payının tamamı sayılır. Kule'nin ev klasörüne yazılabildiğini kontrol edin. Bütçe yoksa satır `(…)` ile biter. |
| `kule: what the run spent could not be read (…), so the <amount> set aside for it stays counted` | Kule çalıştırmanın ajanının bildirdiğini okuyamadı. Yukarıdaki gibi, çalıştırmanın payının tamamı sayılır. |
| `kule: Kule could not put back the protected paths the run changed: …` | Commit etmeden önce değişikliklere [kule status](status.md) ile bakın ya da onları [geri alın](rollback.md). |
| `kule: what the sandbox refused cannot be read: …` / `… could not be recorded: …` | Reddedilenler listelenmedi ya da ajanın bir sonraki çalıştırması ve uygulamanın Timeline'ı için saklanmadı. |
| `kule: what the run changed cannot be shown: …` / `the connections the run made …` / `what the run wrote in your home …` / `the policy …` / `what the run spent …` | Kule o kısmı geri okuyamadı (örneğin kopyada okunamayan bir klasör). Geri kalanı, reddedilenler dahil, yine anlatılır; varsa çalıştırmanın kendi hatası ardından gelir. |
| `kule: Kule could not read the extensions' MCP servers (…); this run goes without them` | [kule ext mcp](ext.md) ile onayladıklarınız okunamıyor, bu yüzden ajan bu çalıştırmada uzantılardan hiçbir MCP sunucusu almaz. Kule'nin ev klasörünün okunabildiğini kontrol edin, sonra onları yeniden onaylayın. |
| `kule: the run ended, but Kule could not do all of the above` | Çalıştırmanın kendisi iyi gitti; yukarıdaki satırlardan biri Kule'nin neyi yapamadığını söyler. |
| `· Kule could not write the agent's log (…); what the agent writes to it is not kept for the rest of this run` | Hata değil, bir not: ajan devam eder, ama o andan sonraki mesajları `agent.log` içinde olmaz. Kule'nin ev klasörüne yazılabildiğini kontrol edin. |
| `kule: <agent> reported no cost, so the task's budget does not count this run` | Hata değil, bir uyarı: abonelikle oturum açmış ajanlar yalnızca token bildirebilir ve bütçe, bildirilmeyeni tutamaz. |

Bazı Linux sistemlerinde, daha zayıf yalıtımı kabul edene kadar hiçbir ajan
çalışmaz; bkz. [kule isolation](isolation.md).

## Uygulamada

Prompt'u **Agent** panelinde yazın (Mod+L imleci oraya koyar), altından
ajanı seçin ve Enter'a ya da **Run**'a basın. Uygulama ayrıca her araç
çağrısının çıktısını akarken gösterir, ajanın sorularını cevaplamanızı
sağlar ve bir **Stop** düğmesi (Mod+Shift+K) sunar. Bkz. [Live](../app/live.md).

## Ayrıca bakın

- [kule exec](exec.md): ajan yerine sandbox içinde tek bir komut çalıştırmak.
- [kule steps](steps.md), [kule status](status.md), [kule commit](commit.md)
- [Ajanlar](../using-agents.md), [Bütçeler ve sınırlar](../concepts/budgets-and-limits.md)
