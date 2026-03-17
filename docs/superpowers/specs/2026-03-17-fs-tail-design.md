# `fs.tail` — `@lmb/fs` 檔案追蹤功能

## 概述

在 `@lmb/fs` 模組中新增 `tail(path, options?)` 方法，回傳一個無限迭代器，持續追蹤檔案新增的行，類似 `tail -F`。自動偵測檔案輪替（logrotate）並跟隨新檔案。

## API

```lua
local fs = require("@lmb/fs")

-- 基本用法：從檔案尾端開始追蹤
for line in fs.tail("/var/log/nginx/access.log") do
    if line:match("500") then
        -- 處理錯誤行
    end
end

-- 帶選項
for line in fs.tail("/var/log/nginx/access.log", {
    poll_interval = 200,    -- 毫秒，預設 100
    from = "end",           -- "end"（預設）= 從尾端開始 / "start" = 從頭開始
}) do
    print(line)
end
```

### 參數

| 參數 | 型別 | 預設值 | 說明 |
|------|------|--------|------|
| `path` | string | 必填 | 要追蹤的檔案路徑 |
| `options.poll_interval` | number | `100` | 無新資料時的輪詢間隔（毫秒） |
| `options.from` | string | `"end"` | 起始位置：`"end"` 跳過既有內容，`"start"` 從頭讀取 |

### 回傳值

回傳一個 Lua 迭代器函數，供 `for...in` 迴圈使用。每次呼叫產出一行（string），尾端換行已去除（與 `fs.lines()` 行為一致）。

## 行為

| 情境 | 行為 |
|------|------|
| 有新行可讀 | 立即回傳，不 sleep |
| 到達 EOF（無新資料） | sleep `poll_interval` 毫秒後重試 |
| 檔案被輪替（inode 變更或大小縮小） | 以同路徑重新開啟新檔案，從頭讀取 |
| 檔案尚不存在 | 持續輪詢等待檔案出現，出現後開始讀取 |
| 檔案暫時消失（輪替空窗期） | 持續等待，檔案重新出現後繼續 |
| 迴圈中 `break` | 正常離開迴圈。`TailState`（含開啟的 `File` handle）由 `Arc<Mutex<...>>` 持有，在 closure 被 GC 回收時釋放。這是非確定性的，但安全——與 `fs.lines()` 處理方式一致。 |
| 權限不足 | 在呼叫時（迭代器開始前）呼叫 `check_read_permission(path)`。既有的 `canonicalize_for_check` 已能處理檔案不存在的情況（透過正規化父目錄並附加檔名）。 |

## 輪替偵測

每次輪詢週期（到達 EOF 後、sleep 前）：

1. 對路徑執行 stat 取得當前 inode 與檔案大小
2. inode 與上次記錄不同 → 檔案已被輪替 → 重新開啟並從頭讀取
3. 檔案大小小於當前讀取位置 → 檔案被截斷 → seek 回開頭
4. stat 失敗（檔案消失）→ 進入等待模式，持續輪詢直到檔案重新出現

此行為與 GNU coreutils 的 `tail -F`（大寫 F）一致。

**已知限制（TOCTOU 競爭）：** stat 與 read 之間存在一個小窗口，期間可能發生輪替。如果檔案在這兩個操作之間被輪替，可能短暫地從舊的 file descriptor 讀取。這與 GNU `tail -F` 的已知限制相同，是可接受的——下一次輪詢週期會偵測到 inode 變化並自動修正。重新開啟後會再次 stat 以確認 inode 與新開啟的檔案一致。

## 實作

### 方式：非同步輪詢

使用 `add_method` 搭配 `vm.create_async_function` 與 `tokio::time::sleep` 進行輪詢等待。這是必要的，因為 LMB 透過 `call_async` 直接在 Tokio runtime 上執行 Lua 腳本——沒有 `spawn_blocking` 或專用的執行緒池。使用 `std::thread::sleep` 會無限期阻塞 Tokio worker thread。

迭代器 closure 註冊為 async function，使得 `tokio::time::sleep` 能在輪詢週期之間讓出 runtime thread。既有的 `fs.lines()` 使用同步 `add_method` 是因為它在 EOF 時終止；`fs.tail` 無限期運行，不可獨佔 runtime thread。

**為何選擇輪詢而非 inotify/notify：**
- 不引入新依賴，不影響 binary 大小（LMB 以輕量部署為目標）
- 所有平台與檔案系統行為一致（包括 inotify 不工作的 NFS）
- 100ms 輪詢間隔的 CPU 成本可忽略（閒置時每秒 10 次 syscall）
- GNU `tail -f` 本身預設也使用輪詢

### Rust 實作

在 `src/bindings/fs.rs` 的 `FsBinding::add_methods` 中新增方法：

```rust
methods.add_method("tail", |vm, this, (path, options): (String, Option<LuaTable>)| {
    // 權限檢查：傳入完整路徑。check_read_permission -> canonicalize_for_check
    // 已能處理不存在的檔案（正規化父目錄並附加檔名）。
    this.check_read_permission(&path).map_err(LuaError::runtime)?;

    let poll_interval = /* 從 options 取得，預設 100 */;
    let from_end = /* 從 options 取得，預設 true */;

    let state = Arc::new(Mutex::new(TailState::new(path, poll_interval, from_end)));

    // 使用 async function 使 tokio::time::sleep 能讓出 runtime thread
    vm.create_async_function(move |vm, ()| {
        let state = state.clone();
        async move {
            loop {
                let result = {
                    let mut state = state.lock();

                    // 1. 確保檔案已開啟（不存在則回傳 None）
                    state.ensure_open();

                    // 2. 檢查輪替（inode/大小變化）
                    state.check_rotation();

                    // 3. 嘗試讀取一行
                    state.read_line()
                };
                // await 前釋放鎖

                match result {
                    Some(line) => return vm.create_string(&line).map(LuaValue::String),
                    None => {
                        // EOF 或檔案未就緒——非同步 sleep，讓出 runtime thread
                        tokio::time::sleep(Duration::from_millis(poll_interval)).await;
                    }
                }
            }
        }
    })
});
```

**重要事項：**
- `parking_lot::Mutex` 鎖（與 `fs.rs` 其餘部分一致）在 `.await` 點前釋放，避免跨 async sleep 持有鎖。這防止死鎖並允許其他 Lua coroutine 繼續執行。
- `fs.tail()` 要求 Lua 腳本透過 `call_async` 執行（LMB 的正常執行路徑）。若從同步上下文呼叫 async 迭代器會 panic。這與 `io.read` 及其他 async binding 的前提條件相同。

### `TailState` 結構

```rust
struct TailState {
    path: PathBuf,
    reader: Option<BufReader<File>>,
    inode: Option<u64>,      // 上次已知的 inode（首次成功開啟前為 None）
                             // Unix 上使用 std::os::unix::fs::MetadataExt::ino()
    position: u64,           // 目前讀取位置
    poll_interval: u64,      // 毫秒
    from_end: bool,          // 首次開啟時是否 seek 到尾端
}
```

**初始狀態：** `inode` 初始為 `None`。首次成功開啟時記錄 inode，不觸發「偵測到輪替」。後續開啟時比較已儲存的值——不一致表示發生了輪替。

**平台說明：** inode 追蹤使用 `std::os::unix::fs::MetadataExt::ino()`，僅限 Unix。非 Unix 平台上輪替偵測退化為僅檢查大小變化（檔案大小縮小）。這是可接受的，因為 LMB 主要目標平台是 Linux。

### 與既有程式碼的整合

| 面向 | 做法 |
|------|------|
| **權限檢查** | 在呼叫時使用 `check_read_permission(path)`。`canonicalize_for_check` 已能處理不存在的檔案（正規化父目錄並附加檔名）。 |
| **方法註冊** | `add_method("tail", ...)` 回傳 async closure（透過 `vm.create_async_function`），與 `FsBinding::add_methods` 中的既有方法並列 |
| **行尾處理** | 去除尾端 `\n` 和 `\r`，與 `fs.lines()` 及 `FileHandleBinding::read("*l")` 一致 |
| **錯誤處理** | 讀取時的 IO 錯誤透過 `LuaError` 回報，與其他 fs 方法一致 |
| **非同步 runtime** | 使用 `tokio::time::sleep`（非 `std::thread::sleep`），因為 LMB 透過 `call_async` 直接在 Tokio worker thread 上執行 Lua。`Mutex` 鎖在每個 await 點前釋放。 |

### 文件更新

在 `fs.rs` 頂部的模組文件註解中新增：

```
//! - `tail(path, options)` - Follow a file like `tail -F`, returning a line iterator that
//!   yields new lines as they are appended. Automatically follows file rotations.
```

## 測試

所有測試位於 `src/bindings/fs.rs` 的 `mod tests` 中，使用 `tempfile` 建立測試檔案：

1. **基本讀取** — 寫入行到檔案，以 `from = "start"` 呼叫 `tail`，驗證所有行都被產出
2. **等待新行** — tail 一個既有檔案（到達 EOF），從另一個 thread 延遲寫入新行，驗證能收到新行
3. **輪替偵測** — tail 一個檔案，重新命名它，在同路徑建立新檔案並寫入，驗證 tail 跟隨新檔案
4. **檔案不存在** — tail 一個不存在的路徑，從另一個 thread 延遲建立檔案，驗證檔案出現後能收到行
5. **break 正常離開** — tail 一個檔案，在 N 行後 break，驗證無 panic 或資源洩漏
6. **權限不足** — 在沒有讀取權限的情況下 tail，驗證回傳錯誤

## 設定參考

無新增 CLI 旗標或環境變數。設定透過 options table 逐次呼叫指定。
