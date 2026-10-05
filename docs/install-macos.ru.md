# Установка LightSpeed в macOS

> [!WARNING]
> Машинный перевод, не проверенный носителем языка. Основным является [английский текст](install-macos.md).

macOS - платформа, где главное приложение это CLI. GUI собирается для macOS, но не протестирован на реальном железе, поэтому поддерживаемый путь - клиент командной строки (`lightspeed-client`).

---

## Что скачать

Возьмите последний релиз со [страницы Releases](https://github.com/ShibbityShwab/lightspeed/releases/latest). Выберите архив под процессор вашего Mac:

| Ваш Mac | Цель | Файл |
|----------|--------|------|
| Apple Silicon (M1 и новее) | `aarch64-apple-darwin` | `lightspeed-client-...-aarch64-apple-darwin.tar.xz` |
| Intel | `x86_64-apple-darwin` | `lightspeed-client-...-x86_64-apple-darwin.tar.xz` |

Не уверены, какой у вас? Выполните:

```bash
uname -m
```

`arm64` означает Apple Silicon; `x86_64` означает Intel.

---

## Установка

Проще всего через установщик для shell. Он определяет вашу архитектуру и ставит клиент:

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/ShibbityShwab/lightspeed/releases/latest/download/lightspeed-client-installer.sh | sh
```

Или установите вручную из архива:

```bash
# Замените имя файла на то, которое скачали.
tar -xf lightspeed-client-...-aarch64-apple-darwin.tar.xz
sudo mv lightspeed-client /usr/local/bin/
```

Проверьте, что бинарник запускается:

```bash
lightspeed-client --version
```

---

## Права root

Перехватчик в macOS использует `pfctl` (встроенный фильтр пакетов) для перенаправления игрового UDP-трафика. Для `pfctl` нужен root, так что запускайте клиент с `sudo`, когда стартуете перехватчик:

```bash
sudo lightspeed-client --start-interceptor --game rust
```

Обнаружение, прощупывание и диагностика (`--probe-proxies`, `--test-control`, `--check`) root не требуют.

---

## Первый запуск

Клиент сам находит общественные релеи через подписанный реестр. Адрес прокси не нужен.

```bash
# Прощупать общественные релеи и напечатать отчет
lightspeed-client --probe-proxies

# Запустить перехватчик для вашей игры
sudo lightspeed-client --start-interceptor --game rust
```

---

## Проверка работы

**Проверьте обнаружение релеев.** Выполните:

```bash
lightspeed-client --probe-proxies
```

Это выполняет один проход обнаружения и прощупывания и печатает наглядный отчет со списком каждого найденного релея и его задержкой. Вы должны увидеть все восемь общественных релеев (Los Angeles, New Jersey, Singapore, Frankfurt, Tokyo, Mumbai, Madrid, Sydney).

**Проверьте регистрацию в плоскости управления.** Выполните:

```bash
lightspeed-client --test-control
```

Это подключается к плоскости управления QUIC, регистрирует сессию, пингует и отключается, печатая результат каждого шага. Успешная регистрация доказывает, что плоскость управления доступна и аутентификация работает.

**Проверьте окружение.** Выполните:

```bash
lightspeed-client --check
```

Это сообщает о доступности перехватчика, статусе root, определении игр и доступности прокси.

**Проверьте поток пакетов.** Как только перехватчик запущен и ваша игра подключена к серверу, счетчики пакетов у клиента должны расти. Если "Packets Sent" остается на 0, перехватчик пока не видит игровой трафик; см. [Устранение неполадок](troubleshooting.md).

---

## Замечание о Gatekeeper

Если macOS блокирует бинарник с сообщением "cannot be opened because the developer cannot be verified", снимите атрибут карантина:

```bash
xattr -d com.apple.quarantine /usr/local/bin/lightspeed-client
```

Делайте это только если скачали бинарник с официальной страницы Releases.

---

## Дальнейшие шаги

- [Поддерживаемые игры](supported-games.md)
- [Устранение неполадок](troubleshooting.md)
- [Справочник CLI](CLI-REFERENCE.md)
