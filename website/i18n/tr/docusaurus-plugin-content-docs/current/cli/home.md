---
title: kule home
sidebar_label: kule home
description: Bir görevin komutlarının ev dizininize yazdıklarını inceleyin, sonra uygulayın, atın ya da bir uygulamayı geri alın.
---

# kule home

Bir görevin komutlarının **ev dizininize** yazdıklarını (`~/.zshrc`,
`~/.config`, `~` dizininin en üstündeki yeni dosyalar) inceleyin; sonra
uygulayın, atın ya da bir uygulamayı geri alın.

## Kullanım {#synopsis}

```sh
kule home [OPTIONS] <TASK> [COMMAND]
```

| Alt komut | Ne yapar |
| --- | --- |
| [`list`](#kule-home-task-list) | İnceleme için yakalanan yazmaları listeler (varsayılan). |
| [`diff`](#kule-home-task-diff) | Bir değişikliğin dosyanıza ne yapacağını satır satır gösterir. |
| [`apply`](#kule-home-task-apply) | Değişiklikleri gerçek ev dizininize uygular. |
| [`discard`](#kule-home-task-discard) | Değişiklikleri atar. |
| [`undo`](#kule-home-task-undo) | Son uygulamayı geri alır. |

Sıraya dikkat edin: görev, alt komuttan **önce** gelir
(`kule home 1046fb30 diff ~/.zshrc`).

## Açıklama {#description}

Ajanlar ve araçlar ev dizininize sık sık yazar: `git config --global`,
`~/.zshrc`'ye eklenen bir satır, `~/.config` altındaki bir aracın ayarları.
Kule'de bu yazmalar reddedilmez, ama ev dizininize de ulaşmaz. Bir görevin
komutları `HOME` olarak bir **ev katmanı** (home layer, `<task>/home`) görür:
ev dizininizin en üstündeki küçük dosyalar ve `~/.config` orada kopyadır,
geri kalan her şey sizinkine giden bir bağlantıdır. Bir komutun katmana
yazdığı şey **inceleme için yakalanır**. Bkz.
[Ev katmanı](../concepts/home-layer.md).

- Ev dizinindeki bir değişiklik hiçbir zaman bir `kule commit`'in parçası
  olmaz. Onun hakkındaki kararı burada verirsiniz.
- `apply` değişiklikleri gözden geçirdiğiniz hâliyle alır: `kule home`'un
  yazdırdığı digest ile (`--reviewed`) ya da bir terminalde adlandırdığı
  şeylere verdiğiniz evet ile. Önce dosyanızın katman oluşturulduğu andaki
  hâlinde durup durmadığını denetler ve yerine geçtiği şeyi görevin
  `home-kept/` klasöründe saklar. Dosyanızın sahip olmadığı yerde hiçbir şey
  sahibi ya da grubu olarak (setuid, setgid) çalışacak hâle gelmez.
- Kabukların ve araçların açılırken okuyup çalıştırdığı dosyalar (`.zshrc`,
  `.profile`, `.gitconfig`, `~/.config/fish`, `~/.config/autostart`,
  systemd kullanıcı birimleri, …) *runs every time a shell or tool starts*
  (bir kabuk ya da araç her açıldığında çalışır) olarak işaretlenir.
  Bunları uygulamadan önce diff'lerine bakın.
- Değişiklikler beklerken katman, bir çalıştırmadan diğerine olduğu gibi
  kalır. Bekleyen değişiklik kalmayınca bir sonraki çalıştırma ev
  dizininizin o anki hâlinden başlar.
- Katmandaki gizli değere benzeyen ayarlar (`export GITHUB_TOKEN=…`)
  görevin komutlarına maskelenmiş görünür; bir uygulama sizin değerinizi
  geri koyar.

### Yolları yazmak {#writing-paths}

Yolları **`list`'in yazdığı gibi** verin: `~/.zshrc` ya da
`~/.config/demo/conf` gibi. Aşağıdakilerin hepsi aynı dosyayı gösterir:
kabuk `~`'yi ev dizininizin tam yoluna çevirebilir, Kule onu da kabul eder.
Ev dizininizin dışındaki ya da içinde `..` geçen bir yol reddedilir.

```sh
kule home 1046fb30 diff ~/.zshrc             # kabuk ~'yi açar; sorun değil
kule home 1046fb30 diff '~/.zshrc'           # tırnaklı; sorun değil
kule home 1046fb30 diff .zshrc               # ev dizininize göre; sorun değil
kule home 1046fb30 diff /Users/you/.zshrc    # ev dizininizin tam yolu; sorun değil
```

Tüm alt komutlar `--home` alır (sizin değil, Kule'nin ev dizini; bkz.
[genel seçenekler](index.md)).

## kule home TASK list

```sh
kule home <TASK> list [OPTIONS]
```

```console
$ kule home 1046fb30
in your home, caught for review (3; `kule home 1046fb30 apply PATH` takes one there):
  + ~/.config/demo/
  + ~/.config/demo/conf
  ~ ~/.zshrc  (runs every time a shell or tool starts)
home digest: 3b7a…90d2 (to apply them: kule home 1046fb30 apply PATH --reviewed 3b7a…90d2)
```

`+` yeni, `~` değişmiş, `-` silinmiş demektir. Sondaki `/` bir klasörü
gösterir; `->` yeni bir bağlantının nereyi gösterdiğini belirtir. Hiçbir
şey yakalanmadıysa: `nothing written in your home`.

`kule exec`, `kule status` ve uygulamadaki Review aynı listeyi gösterir.

## kule home TASK diff

```sh
kule home <TASK> diff [OPTIONS] <PATH>
```

```console
$ kule home 1046fb30 diff ~/.zshrc
--- ~/.zshrc (yours)
+++ ~/.zshrc (the task's)
@@ line 226 @@
 export GITHUB_TOKEN=‹hidden 3fa9c2›
 . "$HOME/.cargo/env"
+export FOO=1
```

Gizli değere benzeyen her şey iki tarafta da gizlenir: anahtar, token ya
da parola gibi adlandırılmış bir ayarın değeri, Kule'nin sakladığı bir
gizli değer ve token'a benzeyen her şey. Her biri `‹hidden …›` olarak
görünür. Aynı değer o diff'in her yerinde aynı işareti alır; böylece
değişen bir gizli değer yine değişmiş olarak okunur. İşaretler bir diff'ten
diğerine değişir.

Metin olmayan bir dosya için `not text, so not shown`, çok büyük bir dosya
için `too large to show` yazar. Uzun bir diff
`(cut here: the change is longer)` ile biter.

## kule home TASK apply

```sh
kule home <TASK> apply [OPTIONS] [PATHS]...
```

| Argüman / seçenek | Anlamı |
| --- | --- |
| `PATHS...` | Uygulanacak yollar, listelendiği gibi. |
| `--all` | Bu görev için yakalanan her değişiklik. |
| `--reviewed DIGEST` | `kule home`'un gözden geçirdiğiniz değişiklikler için yazdırdığı digest. Yalnızca hâlâ onlar oldukları sürece uygulanırlar. Verilmezse Kule bir terminalde uygulayacaklarını adlandırır ve sorar; terminal yoksa reddeder. |

```console
$ kule home 1046fb30 apply '~/.config/demo' '~/.config/demo/conf' --reviewed 3b7a…90d2
applied ~/.config/demo
applied ~/.config/demo/conf
$ kule home 1046fb30 apply '~/.gitconfig' --reviewed 3b7a…90d2
applied ~/.gitconfig
what they replaced is kept in ~/.kule/tasks/1046fb30/home-kept/00000001792506123456
```

Son satır yalnızca uygulama sizin bir şeyinizin yerine geçtiyse ya da onu
kaldırdıysa görünür. Her uygulama orada, ne zaman yapıldığına göre
adlandırılmış kendi klasörünü alır.

Katman oluşturulduğundan beri dosyanız değiştiyse uygulama reddedilir;
böylece sizin hiçbir şeyiniz görmeden üzerine yazılmaz. Görevin
değişiklikleri artık gözden geçirdikleriniz değilse (komutları yeniden
yazdıysa) uygulama yine reddedilir: yeniden gözden geçirin.

Görevin sandbox'ında hâlâ çalışan şey önce sonlandırılır ve Kule stderr'de
onun adını verir:
`kule: Kule ended <name>, still running in the task's sandbox, to apply its home changes`
(ya da `… <N> processes …: <names>`). `undo` da aynısını yapar,
`to undo its home changes` ile. Bkz. [Süreçler ve sunucular](../concepts/sandbox.md#processes-and-servers).

## kule home TASK discard

```sh
kule home <TASK> discard [OPTIONS] [PATHS]...
```

| Argüman / seçenek | Anlamı |
| --- | --- |
| `PATHS...` | Atılacak yollar, listelendiği gibi. |
| `--all` | Bu görev için yakalanan her değişiklik. |

```console
$ kule home 1046fb30 discard --all
discarded 3 changes
```

## kule home TASK undo

```sh
kule home <TASK> undo [OPTIONS]
```

Son uygulamayı, o yolları o zamandan beri değiştirmediyseniz geri alır.
Değişiklikler sonra yeniden inceleme bekler.

```console
$ kule home 1046fb30 undo
undid ~/.config/demo/conf
undid ~/.config/demo
the changes wait for review again
```

## Hatalar ve çözümleri {#errors-and-fixes}

| Mesaj | Ne yapmalı |
| --- | --- |
| `kule: name the paths (as listed, like ~/.zshrc), or --all` | `apply` ve `discard` yollar ya da `--all` ister. |
| `kule: /etc/hosts is not in your home (/Users/you)` | Yalnızca ev dizininizdeki yollar yakalanır. Yolu `list`'in yazdığı gibi verin. |
| `kule: name a path inside your home, as listed, like ~/.zshrc (not ../x)` | Yol ev dizininizin dışına çıkıyor ya da ev dizininin kendisini gösteriyor. |
| `kule: ~/.bashrc has no change waiting` | Görevin orada bekleyen bir yazması yok; bekleyenleri `kule home TASK` listeler. |
| `nothing to apply at those paths` | Bu yollarda bekleyen bir değişiklik yok; `kule home TASK` ile bakın. |
| `kule: the home changes are not the ones you reviewed any more; review them again` | `kule home` digest'i yazdırdığından beri değiştiler. Yeniden bakın, sonra yenisiyle uygulayın. |
| `kule: no terminal to ask on; review with `kule status` and pass the digest it prints with --reviewed` | `kule home`'un yazdırdığı digest'i `--reviewed` ile verin. |
| `kule: ~/<path> would run as its owner or group (setuid or setgid), …` | Kule bunu ev dizininize asla getirmez. Değişikliği atın. |
| `kule: nothing applied from this task's home to undo` | Geri alınacak bir uygulama yok. |
| `kule: a terminal is open on this task's copy (in Kule's app, or kule shell); close it first, then apply its home changes` | Görevin terminalini kapatın (uygulamadaki sekmeyi ya da `kule shell`'den `exit` ile çıkın), sonra uygulayın. |
| `kule: a terminal is open on this task's copy (in Kule's app, or kule shell); close it first, then undo its home changes` | `undo` için de aynısı. |

## Uygulamada {#in-the-app}

Review, **Writes in your home** (ev dizininize yazılanlar) bölümünü
commit'ten ayrı listeler; her birinin yanında *Show* (diff), *Apply*,
*Discard* ve *Undo* vardır. Her açılışta çalışan bir dosya işaretlenir ve
diff'i hemen açılır.

## Ayrıca bakın {#see-also}

- [Ev katmanı](../concepts/home-layer.md)
- [`kule status`](status.md), [`kule exec`](exec.md)
