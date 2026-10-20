---
title: kule daemon
sidebar_label: kule daemon
description: Ajan çalıştırmalarını ve onların beklediği soruları yöneten Kule daemon'ını ön planda çalıştırın.
---

# kule daemon

Kule **daemon**'ını (arka plan süreci) ön planda çalıştırın. Masaüstü
uygulaması için ajan çalıştırmalarını ve onların beklediği soruları yönetir.

## Kullanım {#synopsis}

```sh
kule daemon [OPTIONS]
```

## Açıklama {#description}

Masaüstü uygulaması, daemon'a açılan bir penceredir. Görevler, ajanların
çalıştırmaları ve bekledikleri sorular daemon'a aittir; bu yüzden:

- pencereyi kapatmak hiçbir şeyi durdurmaz: bir çalıştırma devam eder,
  soruları da süreleri dolana kadar bekler;
- pencereyi yeniden açtığınızda arada olanları görürsünüz.

**Normalde onu hiç kendiniz çalıştırmazsınız.** Uygulama, çalışan bir
daemon yoksa onu başlatır (ayrık olarak, çıktısını Kule'nin ev dizinindeki
`kuled.log` dosyasına yazarak). Hata ayıklarken çıktısını izlemek ya da bir
oturum boyunca çalışır tutmak için elle çalıştırın.

Nasıl davranır:

- **Her ev dizini için bir tane.** Bir kilit alır (`kuled.lock`); aynı ev
  dizini için ikinci bir daemon başlamayı reddeder:
  `a Kule daemon is already running for this home`.
- **Onunla yalnızca siz konuşabilirsiniz.** Kule'nin ev dizinindeki
  `kuled.sock` üzerinden dinler. Ev dizini yalnızca sizin okuyabileceğiniz
  hâle getirilir, soket karşı tarafın kullanıcı kimliğini denetler ve her
  istek, hiçbir sandbox'ın okuyamadığı `kuled.token` dosyasından alınmış taze
  bir token taşımak zorundadır.
- **Boştayken kapanır**: bağlı istemci, çalışan ajan ve bekleyen soru
  olmadan `--idle-secs` (varsayılan olarak on dakika) geçtikten sonra.
- Başlarken, silinen görevlerin kaldırılmayı beklediği `trash/` klasörünü
  boşaltır.

Bazı `kule` komutları (örneğin `kule ext install`), çalışan bir daemon
varsa onun üzerinden gider; böylece uygulama değişikliği hemen duyar.

## Seçenekler {#options}

| Seçenek | Değer | Varsayılan | Anlamı |
| --- | --- | --- | --- |
| `--idle-secs` | saniye | `600` | İstemci, çalışan ajan ve bekleyen soru olmadan bu kadar saniye geçince kapanır. |
| `--home` | yol | `~/.kule` (ya da `KULE_HOME`) | Kule'nin ev dizini. Bkz. [genel seçenekler](index.md). |
| `-h`, `--help` | | | Yardımı yazar. |

## Örnekler {#examples}

Varsayılan ev dizini için ön planda çalıştırın:

```console
$ kule daemon
kule: daemon for /Users/you/.kule
```

Bu satırı standart hataya yazar, sonra kapanana kadar sessiz kalır.

Bir saatlik boşta kalma süresi boyunca açık tutun:

```console
$ kule daemon --idle-secs 3600
```

Denemeler için, kendi daemon'ı olan ikinci bir ev dizini:

```console
$ KULE_HOME=~/kule-test kule daemon
kule: daemon for /Users/you/kule-test
```

Uygulamanın başlattığı bir daemon'ın yazdıklarını okuyun:

```sh
tail -f ~/.kule/kuled.log
```

## Hatalar ve çözümleri {#errors-and-fixes}

| Mesaj | Ne yapmalı |
| --- | --- |
| `a Kule daemon is already running for this home` | Zaten bir tane çalışıyor (belki uygulamanınki). Onu kullanın ya da önce durdurun. |
| `socket path …/kuled.sock is longer than 100 bytes; use a shorter KULE_HOME` | Unix soketlerinin yol uzunluğu sınırı kısadır. Kule'nin ev dizinini daha kısa bir yola koyun. |
| `cannot reach the Kule daemon at …/kuled.sock: …` (uygulamadan ya da bir komuttan) | O ev dizini için çalışan bir daemon yok ya da az önce kapandı. Uygulama kendiliğinden yeniden bir tane başlatır. |

Daemon bir hatada 2 durum koduyla çıkar.

## Uygulamada {#in-the-app}

Uygulama daemon'ı sizin için başlatır ve durdurur. Ajanlar çalışırken
pencere kapatılabilir.

## Ayrıca bakın {#see-also}

- [Pencere](../app/window.md)
- [Dosyalar ve ayarlar](../reference/files.md): `kuled.sock`, `kuled.lock`, `kuled.token`, `kuled.log`.
- [Ortam değişkenleri](../reference/environment.md): `KULE_HOME`.
