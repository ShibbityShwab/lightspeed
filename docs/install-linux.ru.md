# Установка LightSpeed в Linux

Linux - платформа, где главное приложение это CLI. GUI собирается для Linux, но поддерживаемый путь для систем без графики и для опытных пользователей - клиент командной строки (`lightspeed-client`).

---

## Что скачать

Возьмите последний релиз со [страницы Releases](https://github.com/ShibbityShwab/lightspeed/releases/latest). Выберите архив под свой процессор:

| Ваша машина | Цель | Файл |
|--------------|--------|------|
| x86_64 (Intel/AMD) | `x86_64-unknown-linux-gnu` | `lightspeed-client-...-x86_64-unknown-linux-gnu.tar.xz` |
| ARM64 (Ampere, Graviton, Raspberry Pi 4/5) | `aarch64-unknown-linux-gnu` | `lightspeed-client-...-aarch64-unknown-linux-gnu.tar.xz` |

Не уверены, какой у вас? Выполните:

```bash
uname -m
```

`x86_64` означает 64-битный Intel/AMD; `aarch64` или `arm64` означает ARM64.

---

## Установка

Проще всего через установщик для shell. Он определяет вашу архитектуру и ставит клиент:

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/ShibbityShwab/lightspeed/releases/latest/download/lightspeed-client-installer.sh | sh
```

Или установите вручную из архива:

```bash
# Замените имя файла на то, которое скачали.
tar -xf lightspeed-client-...-x86_64-unknown-linux-gnu.tar.xz
sudo mv lightspeed-client /usr/local/bin/
```

Проверьте, что бинарник запускается:

```bash
lightspeed-client --version
```

---

## Права root

Перехватчик в Linux использует `nftables` (или `iptables`) для перенаправления игрового UDP-трафика, а для этого нужен root. Запускайте клиент с `sudo`, когда стартуете перехватчик:

```bash
sudo lightspeed-client --start-interceptor --game rust
```

Обнаружение, прощупывание и диагностика (`--probe-proxies`, `--test-control`, `--check`) root не требуют.

Убедитесь, что `nftables` установлен, если ваш дистрибутив не ставит его по умолчанию:

```bash
# Debian/Ubuntu
sudo apt install nftables

# Fedora/RHEL
sudo dnf install nftables

# Arch
sudo pacman -S nftables
```

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

## Дальнейшие шаги

- [Поддерживаемые игры](supported-games.md)
- [Устранение неполадок](troubleshooting.md)
- [Справочник CLI](CLI-REFERENCE.md)
