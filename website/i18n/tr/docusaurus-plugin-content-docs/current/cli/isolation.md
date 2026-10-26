---
title: kule isolation
sidebar_label: kule isolation
description: Bu bilgisayarın sandbox'taki bir komuta veremediklerini gösterin ve daha azıyla çalışmayı kabul edin ya da reddedin.
---

# kule isolation

Bu bilgisayarın sandbox'taki bir komuta veremediklerini gösterir ve
komutların daha azıyla çalışıp çalışamayacağını seçmenizi sağlar.

## Kullanım {#synopsis}

```sh
kule isolation [OPTIONS]
```

## Açıklama {#description}

Kule, sizin onayınız olmadan bir komutu **amaçladığından daha az yalıtımla**
asla çalıştırmaz. Bazı bilgisayarlarda [sandbox](../concepts/sandbox.md)'ın
bir kısmı kullanılamaz. Yaygın durum, ayrıcalıksız kullanıcı ad alanları
(unprivileged user namespaces) olmayan Linux'tur (Ubuntu 24.04 bunları
varsayılan olarak reddeder). Kule bunları her komuta kendine ait bir ağ,
bir `/tmp`, süreçler ve paylaşılan bellek vermek için kullanır.

Bir şey eksikken ve siz bunu kabul etmediyseniz Kule komutları ve ajanları
çalıştırmayı reddeder ve neyin eksik olduğunu söyler. `kule isolation`
eksikleri gösterir; `--accept-weaker on` komutların bilgisayarın
verebildiğiyle çalışmasına izin verir. Seçiminiz Kule'nin evinde
(`isolation.json`) saklanır.

macOS'ta tek eksik, `servers` [sınırının](limits.md) dinlemelerine izin
verdiği görevin sunucularıdır: macOS sandbox'ı bir sunucunun dinlediği
port'u görür ama adresini görmez. Kule Node, Python ve Ruby sunucularını bu
bilgisayara yönlendirir, diğerlerini de 50 ms'de bir bakarak sandbox'ın
dışından ağınızdan uzak tutar; o arada ağdan gelen bir bağlantı birine
ulaşabilir ve Kule onu o zaman durdurur. `kule isolation`, her görevin
sınırları sunuculara izin verdiğinde bunu gösterir; sunuculara kendi başına
izin veren bir görev ya da gruba bu, çalıştığında söylenir.

Windows'ta kabul edilecek bir şey yoktur: süreç konteyneri bir komuta
yalıtımının tamamını verir ya da hiçbirini. Kullanılamadığı yerde
(26100.9278 güncellemeli Windows 11 24H2'den eski bir Windows, ya da bir
hizmetin veya uzak bir kabuğun oturumu) `kule isolation`, sandbox'lı
komutların bu bilgisayarda neden reddedildiğini söyler; `--accept-weaker
on` hiçbir şeyi değiştirmez.

## Seçenekler {#options}

| Seçenek | Değer | Varsayılan | Anlamı |
| --- | --- | --- | --- |
| `--accept-weaker` | `on` ya da `off` | `off` | `on`, komutları bu bilgisayarın verebildiğiyle çalıştırır; `off`, bir şey eksik olduğu sürece onları reddeder. |
| `--home` | yol | `~/.kule` (ya da `KULE_HOME`) | Kule'nin evi. Bkz. [genel seçenekler](index.md). |
| `-h`, `--help` | | | Yardımı yazdırır. |

## Örnekler {#examples}

Her şeyi veren bir bilgisayar (macOS, ya da kullanıcı ad alanları ve yeni
bir Landlock sürümü olan Linux):

```console
$ kule isolation
this computer gives a sandboxed command all of its isolation
```

Ayrıcalıksız kullanıcı ad alanları olmayan bir Linux bilgisayar:

```console
$ kule isolation
this computer cannot give a sandboxed command:
  - shared memory of its own: it could open the System V segments and queues of your other programs
  - a network of its own: only the proxy's port is checked, on any address, and datagrams pass
  - agents' settings kept read-only while it runs: they are put back after it instead
  - a /tmp of its own: it shares /tmp with your other programs and tasks
  - processes of its own: what it leaves running is found by a guess, which one could slip
not accepted: commands are refused (kule isolation --accept-weaker on)
```

Satırların tam hali çekirdeğe bağlıdır: hangi ad alanlarına izin verdiği ve
hangi Landlock sürümüne sahip olduğu.

Kabul edin:

```console
$ kule isolation --accept-weaker on
this computer cannot give a sandboxed command:
  ...
accepted: commands run with what it gives
```

Yeniden reddetmeye dönün:

```console
$ kule isolation --accept-weaker off
```

## Daha iyisi: bilgisayara Kule'nin ihtiyacını verin {#better-give-the-computer-what-kule-needs}

Ubuntu 24.04 ve sonrasında daha azını kabul etmek yerine ayrıcalıksız
kullanıcı ad alanlarına izin verin:

```sh
sudo sysctl kernel.apparmor_restrict_unprivileged_userns=0
```

Ayarın yeniden başlatmadan sonra da kalması için aynı ayarı `/etc/sysctl.d/`
altındaki bir dosyaya yazın. Ağ kuralları ayrıca Linux 6.7 ya da sonrasını
(Landlock ABI 4) gerektirir. Bkz. [Platformlar](../platforms.md).

## Hatalar ve çözümler {#errors-and-fixes}

| Mesaj | Ne yapmalı |
| --- | --- |
| `kule: maybe: on or off` | `--accept-weaker`, `on` ya da `off` alır. |
| `this computer cannot give the command …. Kule does not run it less isolated without your say: accept weaker isolation in Settings → Policy, or with kule isolation --accept-weaker on` (`exec` ya da `run`'dan) | Yukarıdaki gibi kullanıcı ad alanlarına izin verin ya da daha zayıf yalıtımı bilerek kabul edin. |

## Uygulamada {#in-the-app}

*Settings → Policy* neyin eksik olduğunu listeler ve siz **Run agents with
what this computer gives** (ajanları bu bilgisayarın verebildiğiyle çalıştır)
kutusunu işaretleyene kadar hiçbir ajan çalışmaz. Bu geçerli olduğu sürece
başlık çubuğunda **Weaker isolation** (daha zayıf yalıtım) yazar.

## Ayrıca bakın {#see-also}

- [Sandbox](../concepts/sandbox.md), [Platformlar](../platforms.md), [Sorun giderme](../troubleshooting.md)
