# 커널 알려진 문제 (Kernel Known Issues)

이 머신(Dell XPS 15 9570 / Pop!_OS 24.04)에서 확인된 커널 버전별 문제와 권장 버전을 기록한다.
새 커널 업데이트 후 시스템이 불안정해지면 이 표를 먼저 확인하고, 필요 시 권장 버전으로 롤백한다.

## 버전 권장표

| 커널 버전 | 상태 | 비고 |
|---|---|---|
| `7.0.9-76070009` | ✅ **권장 (known-good)** | 반복 프리즈 없음. 롤백 대상. |
| `7.0.11-76070011` | ❌ **회피** | slab shrinker 손상으로 반복 하드 프리즈 (아래 참조) |

> **현재 권장 부팅 커널: `7.0.9-76070009-generic`**
> System76이 7.0.11 이후 수정 커널을 내면 재평가한다.

---

## #1 — 7.0.11-76070011: slab shrinker 손상으로 반복 하드 프리즈

- **최초 진단:** 2026-06-17 (전 세션) / **재확인:** 2026-06-18
- **영향 커널:** `7.0.11-76070011` (System76 / Pop!_OS 24.04)
- **하드웨어:** Dell XPS 15 9570, BIOS 1.23.0, Intel + NVIDIA GTX 1050 Ti (Optimus)

### 증상
- 사용 중 시스템 전체가 멈추고(하드 프리즈) **강제 재부팅** 외 복구 불가.
- 정상 종료 로그 없이 journal이 특정 시각에 뚝 끊김.

### 커널 oops 시그니처
```
Oops: general protection fault
RIP: 0010:sio_ite_8872_probe+0x1f3/0x560 [parport_pc]
Call Trace:
  do_shrink_slab → shrink_slab → shrink_one → shrink_many
  → shrink_node → balance_pgdat → kswapd
```
- RIP은 매번 `parport_pc`의 `sio_ite_8872_probe`에 착지하지만, 실제 호출 경로는 **항상 메모리 회수(`kswapd0 → shrink_slab`)**.
- 죽는 순간 돌던 프로세스는 매번 다름: `kswapd0`, `pactl`, `nvidia-smi`, `tokio-rt-worker`, `waveterm` 등.
- → **slab shrinker 리스트가 손상되어 함수 포인터가 엉뚱한 커널 주소로 점프**하는 메모리 손상 cascade. `parport_pc`는 우연히 착지한 **피해자**일 뿐 진범 아님.

### 근본 원인: 커널 회귀(regression)
- 크래시 onset이 패키지 업그레이드와 정확히 일치:
  ```
  2026-06-12 12:27  linux-system76         7.0.9-76070009 → 7.0.11-76070011
  2026-06-12 12:27  system76-driver-nvidia 24.04.19       → 24.04.20
  ```
- 6/12 이전(7.0.9): 4개 부팅 연속 클린. 6/12 이후(7.0.11): 크래시 시작.
- **MCE / EDAC 하드웨어 오류 흔적 없음** → 배드램 아님, 커널/드라이버 회귀.
- 7.0.11도 결정적이진 않음 — 짧은 부팅은 클린, **업타임·메모리압력 누적 시 확률적으로** 발생.

### 조치

**1) (적용됨) parport 모듈 차단 — band-aid**
이 노트북엔 물리 패러럴 포트가 없는데 `cups-filters`가 쓸모없이 로드하던 모듈. 충돌 착지점 1개 제거(무해, 진범 아님).
```bash
# /etc/modprobe.d/blacklist-parport.conf
blacklist parport_pc
blacklist ppdev
blacklist lp
blacklist parport
```

**2) (권장) 커널 7.0.9로 롤백 — 진짜 해법**
known-good 커널로 기본 부팅을 고정. 이미지·모듈·nvidia DKMS 모듈 모두 잔존하여 즉시 부팅 가능.
```bash
# 7.0.9로 고정
sudo kernelstub \
  --kernel-path /boot/vmlinuz-7.0.9-76070009-generic \
  --initrd-path /boot/initrd.img-7.0.9-76070009-generic \
  --verbose

# 확인 (vmlinuz-7.0.9... 가 보이면 성공)
sudo kernelstub -p | grep -iE "kernel|initrd"

# 이후 재부팅 1회
```
- **되돌리기:** 위 경로를 `7.0.11-76070011`로 바꿔 다시 실행.
- 이 고정은 **다음 커널 업데이트 전까지** 유지됨(`apt`로 새 커널 설치 시 kernelstub가 최신으로 재지정). 업데이트 시 이 문서를 다시 확인할 것.

### 진단에 쓴 명령 (재현용)
```bash
journalctl --list-boots                              # 부팅 목록/끊긴 시점
journalctl -b -1 | tail -80                          # 프리즈 직전 로그
journalctl -b -1 | grep -A40 "Oops\|RIP\|Call Trace" # oops 전체
# 부팅별 첫 oops 시점으로 onset 추적 → dpkg.log와 대조
grep -E "upgrade |install " /var/log/dpkg.log* | grep -iE "linux-image|nvidia"
journalctl -b -1 -b -2 -b -3 | grep -iE "mce:|machine check|EDAC"  # 하드웨어 오류 배제
```
