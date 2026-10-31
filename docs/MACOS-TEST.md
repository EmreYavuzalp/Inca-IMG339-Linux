# IMG-339 macOS denemesi

Bu sürüm deneyseldir ve henüz gerçek bir Mac'te fareyle denenmedi. Amaç, neyin
çalışıp neyin çalışmadığını görmek. Yaklaşık 10 dakika sürer. Denemeler farenin
ayarlarını değiştirir; son adımdaki komut bunları fabrika ayarlarına döndürür.

## Kurulum

1. Fareyi takın.
2. `IMG-339.app` dosyasını **Uygulamalar** klasörüne sürükleyin.
3. **Terminal**'i açın ve şunu çalıştırın. Apple'a kayıtlı bir geliştirici imzası
   olmadığı için macOS uygulamayı aksi halde engeller:

   ```sh
   xattr -dr com.apple.quarantine /Applications/IMG-339.app
   ```

## Komut satırı denemesi

Çıktıları kopyalayıp gönderin:

```sh
A=/Applications/IMG-339.app/Contents/MacOS/img339ctl
$A status
$A dpi 1 --active
$A status
```

- İlk komut `Mouse:` satırında bir yol göstermeli.
- macOS "Giriş İzleme" izni isterse verin (Sistem Ayarları > Gizlilik ve Güvenlik >
  Giriş İzleme) ve komutu tekrarlayın. Bu izin istendiyse raporda belirtin.
- İmleç donar ya da fare çalışmaz hale gelirse fareyi çıkarıp takın ve bunu da yazın.

## Uygulama denemesi

`IMG-339` uygulamasını açın ve şunları kontrol edin:

| # | Deneme | Beklenen |
|---|---|---|
| 1 | Pencere açıldı mı, altta ne yazıyor? | "Fare bağlı" |
| 2 | Bir seviyenin DPI değerini değiştirip **Uygula** | İmleç hızı değişir |
| 3 | Işık sekmesinde LED modunu değiştirip **Uygula** | Farenin ışığı değişir |
| 4 | Farenin DPI tuşlarına basın | "Şu anki DPI" değişir, yuvarlak seviyeler arasında sıçrar |
| 5 | Pencere açıkken fareyi kullanın | Fare ve klavye normal çalışır |
| 6 | Tuşlar sekmesinde yan tuşa "Sesi aç" atayıp **Uygula**, servisi başlatın, tuşa basın | Ses yükselir |
| 7 | Yan tuşa "Kopyala" atayın, bir metin seçip tuşa basın, başka yere yapıştırın | Metin kopyalanır |

Fare klavye olarak da göründüğü için macOS "Klavye Kurulum Yardımcısı" penceresi
açabilir; kapatabilirsiniz. Açıldıysa bunu da yazın.

## Bitirirken

```sh
$A reset
```

## Sonucu bildirin

Şunları bir [issue](https://github.com/EmreYavuzalp/Inca-IMG339-Linux/issues) ile gönderin:

- Mac modeli (Intel mi, Apple Silicon mı) ve macOS sürümü
- Komutların çıktıları
- Tablodaki her deneme için çalıştı ya da çalışmadı, varsa ekran görüntüsü
