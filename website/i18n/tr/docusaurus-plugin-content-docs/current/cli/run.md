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
ebeveynine ne olmuş olursa olsun.

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
| `· <note>` | Kule'den ya da ajandan bir not; örneğin ona verilen bir izin. |
| `N file changes: +a ~m -d` | Kopyada olup çalışma klasöründe olmayanlar: oluşturulan, değiştirilen, silinen. Her dosyanın içinde değişen fonksiyonlar, bölümler ya da anahtarlar. Bkz. [kule status](status.md). |
| `git:` | Depoya ne olduğu: dal hareketleri, yeni nesneler, config ya da hook değişiklikleri. Bkz. [Git](../concepts/git.md). |
| `net:` | Çalıştırmanın yaptığı her bağlantı ve geçip geçmediği. |
| `in your home, caught for review` | Ev klasörünüze yapılan ve ev katmanında bekleyen yazmalar. Bkz. [kule home](home.md). |
| `sandbox: refused …` | Sandbox'ın reddettikleri (macOS bunları listeler; Linux'ta komut yalnızca bir hata alır). |
| `policy:` | Değişiklikler hakkındaki karar: *passes on its own* (kendiliğinden geçer), *waits for approval* (onay bekler) ya da *denied* (reddedildi), her birinin arkasındaki kuralla birlikte. |
| `spent:` | Görevin çalıştırmalarının, ajanların bildirdiğine göre, şimdiye kadar ne kadar tuttuğu ve bütçesinden ne kaldığı. |
| `took:` | Çalıştırmanın ne kadar sürdüğü. |

Çıktı bir terminal değilse her olay kendi düz satırında yazılır; bu,
günlüğe kaydetmeyi kolaylaştırır.

## Hatalar ve çözümleri

| Mesaj | Ne yapmalı |
| --- | --- |
| `kule: no agent <name>; known: claude-code, gemini, codex` | Adı kontrol edin. Kendi ajanlarınızı [kule agents](kule-agents.md) ile ekleyin. |
| `kule: agent <name> cannot run: <why>` | Ajan eksik, oturum açılmamış ya da (kendi profilleriniz için) henüz onaylanmamış. Nedenini görmek için `kule agents` çalıştırın. |
| `kule: task <id> has spent its budget; raise it or turn it off to run again` | `kule budget TASK <amount>` ile artırın ya da `kule budget TASK off` ile kapatın. Tükenen bir grubun ya da günlük bütçeyse mesaj onun adını verir. |
| `kule: the task has used its time: its runs took N minutes together; …` | Görevin süre sınırını [kule limits](limits.md) ile artırın ya da kaldırın. |
| `kule: an agent is at work in task <id> …` | Görev başına tek ajan. Bekleyin ya da başka bir görev açın. |
| `kule: a terminal is open on this task's copy (in Kule's app, or kule shell); close it first, then start work` | Görevde bir terminal (uygulamanınki ya da `kule shell`) açık. Önce onu kapatın. |
| `kule: Kule opened task <id>'s copy in <app> outside the sandbox; …` | O terminal uygulamasını ya da editörü kapatın, sonra [kule closed](closed.md) `TASK` çalıştırın. |
| `kule: <error> (see <home>/tasks/<id>/agent.log)` | Ajan başarısız oldu. Kendi mesajları `agent.log` içinde: son 8 MB'ı, ondan önceki 8 MB'ı da `agent.log.1` içinde. |
| `kule: stopped: the agent sent a message larger than 16 MiB (see …)` | Ajan tek mesajda Kule'nin aldığından fazlasını söyledi. Daha dar bir istemle yeniden çalıştırın. |
| `kule: stopped: the agent left more than 1024 tool calls open (see …)` | Ajan araç çağrılarını bitirmeden yenilerini başlattı. |
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
