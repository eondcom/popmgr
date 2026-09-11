# 리눅스에서 켄징턴 트랙볼 블루투스로 연결하기 (Kensington Expert Wireless)

> 드라이버를 찾아 헤매다가 정작 원인은 다른 데 있었던 이야기. Pop!_OS 24.04 / BlueZ 5.72
> 에서 실제로 해결한 기록입니다. 우분투 계열이면 그대로 적용됩니다.

## 세 줄 요약

1. **드라이버는 필요 없습니다.** 켄징턴 트랙볼은 표준 USB HID 라 커널이 알아서 잡습니다.
2. **페어링 모드 진입은 상단 버튼 4개를 동시에 3초간** 누르는 것입니다. 바닥에 페어링
   버튼이 따로 없습니다.
3. `bluetoothctl` 을 **명령마다 새로 실행하면 페어링이 안 됩니다.** 한 세션을 유지한 채
   scan → pair → trust → connect 를 순서대로 넣어야 합니다.

---

## 1. 먼저 확인할 것 — 정말 드라이버 문제인가

"켄징턴 트랙볼 리눅스 드라이버"를 검색하면 시간만 버립니다. 애초에 존재하지 않습니다.
켄징턴이 만드는 KensingtonWorks 는 Windows/macOS 전용이고 리눅스판이 없습니다.

대신 리눅스는 표준 HID 로 잡습니다. 2.4GHz 동글을 꽂으면 이렇게 나옵니다:

```bash
lsusb | grep -i kensington
# Bus 001 Device 010: ID 047d:8018 Kensington Expert Wireless Trackball Mouse (K72359WW)

grep -A5 -i kensington /proc/bus/input/devices
# N: Name="Kensington Expert Wireless TB Mouse"
# N: Name="Kensington Expert Wireless TB Consumer Control"
```

여기까지 나오면 **드라이버는 정상입니다.** 안 되는 건 다른 문제입니다.

## 2. 함정 1 — `lsusb` 의 모델명은 동글 이름이지 본체 사양이 아니다

`lsusb` 가 `K72359WW` 라고 알려주길래 저는 제품 사양서를 찾아보고 "이 모델은 바닥에
페어링 버튼이 있다"고 결론 내렸습니다. **틀렸습니다.**

`047d:8018` 은 **USB 동글이 자기를 소개하는 문자열**입니다. 본체가 블루투스를 지원하는지,
버튼이 어디 붙어 있는지는 여기서 알 수 없습니다. 실물을 봐야 합니다.

## 3. 함정 2 — 페어링 모드 진입 방법

Kensington Expert 계열은 **상단 버튼 4개를 동시에 3초간** 누르면 페어링 모드로 들어갑니다.
바닥을 아무리 뒤져도 페어링 버튼은 없습니다.

제대로 들어가면 블루투스 스캔에 **두 개**가 동시에 뜹니다:

```
Device C0:31:F2:BB:80:4A ExpertBT5.0    <- 이쪽으로 연결
Device 12:34:D0:74:62:20 ExpertBT3.0
```

`ExpertBT5.0` 이 블루투스 5.0 채널, `ExpertBT3.0` 이 구형 채널입니다. **5.0 쪽으로
페어링하세요.**

이름에 `Kensington` 이 안 들어갑니다. 그래서 "켄징턴으로 검색되겠지" 하고 기다리면
영원히 못 찾습니다. 저는 이것 때문에 한참 헤맸습니다.

확실하게 트랙볼인지 확인하려면:

```bash
bluetoothctl info C0:31:F2:BB:80:4A | grep -E "Icon|Appearance|UUID"
# Appearance: 0x03c2 (962)      <- 마우스
# Icon: input-mouse
# UUID: Human Interface Device  (00001812-...)  <- HID
```

## 4. 함정 3 (핵심) — `bluetoothctl` 을 반복 실행하면 페어링이 실패한다

이게 제일 오래 잡아먹은 부분입니다. 이렇게 하면 **안 됩니다**:

```bash
# 안 되는 방법
bluetoothctl --timeout 30 scan on
bluetoothctl pair C0:31:F2:BB:80:4A     # <- 여기서 조용히 실패
```

`Attempting to pair with ...` 까지만 뜨고 결과가 안 나옵니다. 로그를 보면 이러고 있습니다:

```
Attempting to pair with C0:31:F2:BB:80:4A
[DEL] Device 12:34:D0:74:62:20 ExpertBT3.0
[DEL] Device 56:17:FA:8A:77:12 ...
```

`scan on` 프로세스가 끝나는 순간 BlueZ 가 **발견 캐시를 통째로 비웁니다(`DEL`)**. 그
직후 `pair` 를 부르면 대상이 이미 사라진 뒤라 페어링이 중단됩니다.

### 되는 방법 — 한 세션을 유지한다

```bash
#!/usr/bin/env bash
MAC=C0:31:F2:BB:80:4A

coproc BTC { stdbuf -oL bluetoothctl 2>&1; }
exec 3>&"${BTC[1]}"

echo "power on" >&3;      sleep 1
echo "agent on" >&3;      sleep 1
echo "default-agent" >&3; sleep 1
echo "scan on" >&3;       sleep 10   # 기기가 잡힐 때까지

echo "pair $MAC" >&3;     sleep 12
echo "trust $MAC" >&3;    sleep 3    # trust 를 해야 다음부터 자동 재연결
echo "connect $MAC" >&3;  sleep 10

echo "scan off" >&3
echo "quit" >&3
```

`coproc` 으로 `bluetoothctl` 을 하나 띄워두고 명령을 순서대로 흘려 넣는 구조입니다.
스캔이 살아 있는 동안 페어링이 끝나므로 캐시가 날아가지 않습니다.

> GUI(GNOME/COSMIC 블루투스 설정)로도 됩니다. GUI 는 내부적으로 스캔을 계속 유지하기
> 때문입니다. CLI 로 할 때만 이 함정에 빠집니다.

## 5. 성공 확인

```bash
bluetoothctl info C0:31:F2:BB:80:4A | grep -E "Paired|Trusted|Connected"
#   Paired: yes
#   Trusted: yes
#   Connected: yes

grep "N: Name" /proc/bus/input/devices | grep Expert
# N: Name="ExpertBT5.0 Mouse"
# N: Name="ExpertBT5.0 Consumer Control"
```

`Trusted: yes` 가 중요합니다. 이게 있어야 다음부터 전원만 켜면 자동으로 붙습니다.

## 6. 안 될 때 체크리스트

**동글이 꽂혀 있으면 빼세요.** 동글이 붙어 있으면 트랙볼이 2.4GHz 채널을 계속 잡고
있어서 블루투스 광고를 안 냅니다. `lsusb | grep -i kensington` 에 여전히 나온다면
트랙볼은 아직 동글 모드입니다.

**주변에 HID 기기가 잡히는지 봅니다.** 스캔에 아무 마우스도 안 잡힌다면 기기가 광고를
안 하는 상태입니다. 호스트 문제가 아닙니다:

```bash
bluetoothctl devices | awk '{print $2}' | while read -r m; do
  bluetoothctl info "$m" | grep -q 00001812 && echo "HID: $m"
done
```

**호스트 블루투스 자체는 이렇게 확인합니다:**

```bash
bluetoothctl show | grep -E "Powered|Pairable"   # 둘 다 yes 여야 함
rfkill list bluetooth                            # blocked: no 여야 함
systemctl is-active bluetooth                    # active
```

**배터리.** 동글로는 멀쩡히 되는데 블루투스만 안 되는 경우, 블루투스 광고가 전력을 더
써서 배터리가 약하면 광고 단계에서 실패합니다.

## 7. 블루투스 ↔ 동글 전환을 스크립트로 할 수 있나?

**없습니다.** 채널 전환은 기기 펌웨어가 물리 버튼으로만 처리합니다. 로지텍 Unifying
처럼 호스트에서 채널을 제어하는 규격(Solaar 같은 도구)이 켄징턴에는 없습니다.

PC 쪽에서 할 수 있는 건 "이미 페어링된 기기에 연결/해제"까지입니다:

```bash
bluetoothctl connect C0:31:F2:BB:80:4A
bluetoothctl disconnect C0:31:F2:BB:80:4A
```

## 8. 버튼 재매핑은 어떻게?

KensingtonWorks 가 없으니 리눅스 기본 도구를 씁니다.

- **input-remapper** (GUI, 추천): `sudo apt install input-remapper`
- **libinput 스크롤**: 버튼 하나를 누른 채 볼을 굴려 스크롤 (`ScrollMethod=button`)
- **udev hwdb**: 저수준 키코드 재매핑

## 덧 — 같이 겪은 다른 문제

리얼포스 키보드가 USB 로 안 잡히길래 드라이버를 의심했는데, 커널 로그가 답을 알려줬습니다:

```
usb 1-2: unable to read config index 0 descriptor/start: -32
usb 1-2: can't read configurations, error -32
usb usb1-port2: unable to enumerate USB device
```

`error -32` 는 EPIPE, 기기가 자기소개(디스크립터)를 못 보냈다는 뜻입니다. **케이블이
충전 전용이거나 단선일 때 정확히 이 증상**이 납니다. 케이블을 바꾸니 바로 잡혔습니다.

입력기기가 안 잡히면 드라이버를 뒤지기 전에 `journalctl -k --since "-10 min" | grep usb`
부터 보세요. 물리 문제인지 아닌지 바로 나옵니다.

---

## 정리

| 증상 | 진짜 원인 |
|---|---|
| 블루투스 스캔에 트랙볼이 안 뜬다 | 페어링 모드 미진입 (버튼 4개 3초) |
| 켄징턴 이름으로 안 찾아진다 | `ExpertBT5.0` 으로 광고함 |
| `pair` 가 조용히 실패한다 | `bluetoothctl` 재실행 → 발견 캐시 `DEL` |
| 매번 다시 페어링해야 한다 | `trust` 를 안 함 |
| 블루투스로 아예 전환이 안 된다 | 동글이 꽂혀 있음 |
| 드라이버가 없는 것 같다 | 드라이버는 원래 필요 없음 (표준 HID) |

환경: Pop!_OS 24.04 LTS / 커널 7.1.5 / BlueZ 5.72 / Kensington Expert Wireless Trackball
