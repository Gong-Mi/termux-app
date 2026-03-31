# 用 Linux 内核方式解决 Rust 集成死锁问题

## 🐧 Linux 内核的并发解决方案

### Linux 内核面临的类似问题

```
内核中断处理程序 (Interrupt Handler)
    ↓
获取自旋锁 (spin_lock)
    ↓
处理数据
    ↓
回调上层子系统 (call_usermodehelper)
    ↓
上层又调用内核函数
    ↓
尝试获取锁... ← 【死锁!】
```

### Linux 的解决方案

---

## 方案 1: 中断下半部 (Bottom Half) → 事件队列 ✅

**Linux 方式:**
```c
// 中断上半部 - 快速处理，不阻塞
irqreturn_t my_interrupt(int irq, void *dev_id) {
    // 1. 禁用中断
    disable_irq(irq);
    
    // 2. 标记需要处理
    schedule_tasklet(&my_tasklet);
    
    // 3. 立即返回
    return IRQ_HANDLED;
}

// 中断下半部 - 稍后处理，可以阻塞
void my_tasklet(unsigned long data) {
    // 4. 获取锁
    mutex_lock(&my_mutex);
    
    // 5. 处理数据
    process_data();
    
    // 6. 释放锁
    mutex_unlock(&my_mutex);
    
    // 7. 启用中断
    enable_irq(irq);
}
```

**对应到 Termux:**
```rust
// Rust IO 线程 - 上半部
while running {
    match file.read(&mut buffer) {
        Ok(n) => {
            // 1. 快速获取锁，处理数据
            let events = {
                let mut engine = context.lock.write().unwrap();
                engine.process_bytes(&buffer[..n]);
                engine.take_events()  // ← 取出事件
            }; // ← 立即释放锁
            
            // 2. 调度"下半部"处理回调
            schedule_callback(events);  // ← 类似 tasklet
        }
    }
}

// 回调处理 - 下半部
fn flush_events_to_java() {
    // 3. 在锁外回调 Java
    for event in events {
        env.call_method(obj, "onEvent", "()V", &[]);
    }
}
```

**状态:** ✅ 已实现

---

## 方案 2: RCU (Read-Copy-Update) → 读写锁优化

**Linux 方式:**
```c
// 读侧 - 无锁，零开销
rcu_read_lock();
data = rcu_dereference(ptr);
use(data);
rcu_read_unlock();

// 写侧 - 复制 - 修改 - 替换
new_data = kmalloc(sizeof(*new_data));
*new_data = *old_data;
new_data->field = new_value;
rcu_assign_pointer(ptr, new_data);

// 等待所有读侧完成
synchronize_rcu();

// 释放旧数据
kfree(old_data);
```

**对应到 Termux:**
```rust
// 读侧 - Java 获取颜色
pub fn get_colors(&self) -> [u32; 259] {
    // 无锁读取（使用原子操作）
    self.colors.load(Ordering::Relaxed)
}

// 写侧 - Rust 更新颜色
pub fn reset_colors(&self) {
    // 复制 - 修改
    let new_colors = DEFAULT_COLORS;
    
    // 原子替换
    self.colors.store(new_colors, Ordering::Release);
    
    // 通知 Java（异步）
    self.notify_colors_changed();
}
```

**优势:**
- ✅ 读侧零开销
- ✅ 无锁竞争
- ✅ 适合读多写少场景

**状态:** ⚠️ 部分实现（使用 AtomicBool 代替）

---

## 方案 3: 无锁编程 (Lock-free) → 原子操作

**Linux 方式:**
```c
// 使用原子操作代替锁
atomic_t counter = ATOMIC_INIT(0);

// 增加计数（无锁）
atomic_inc(&counter);

// 比较 - 交换 (CAS)
if (atomic_cmpxchg(&ptr, old, new) == old) {
    // 成功
} else {
    // 失败，重试
}
```

**对应到 Termux:**
```rust
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

pub struct SessionCoordinator {
    // 使用原子操作代替 Mutex
    pkg_lock: AtomicBool,
    pkg_lock_owner: AtomicUsize,
}

impl SessionCoordinator {
    // 无锁获取锁
    pub fn try_acquire_pkg_lock(&self, session_id: usize) -> bool {
        self.pkg_lock.compare_exchange(
            false, true,
            Ordering::SeqCst,
            Ordering::SeqCst
        ).is_ok()
    }
    
    // 无锁释放锁
    pub fn release_pkg_lock(&self, session_id: usize) {
        self.pkg_lock.store(false, Ordering::SeqCst);
    }
}
```

**状态:** ✅ 已实现

---

## 方案 4: 信号量 (Semaphore) → 限流

**Linux 方式:**
```c
struct semaphore pkg_sem;
sema_init(&pkg_sem, 1);  // 初始值为 1（互斥锁）

// 获取信号量（可能阻塞）
down(&pkg_sem);

// 执行 pkg 操作
run_pkg_upgrade();

// 释放信号量
up(&pkg_sem);
```

**对应到 Termux:**
```rust
use tokio::sync::Semaphore;
use std::sync::Arc;

pub struct PkgManager {
    sem: Arc<Semaphore>,
}

impl PkgManager {
    pub async fn run_pkg_upgrade(&self) -> Result<()> {
        // 获取许可（可能等待）
        let permit = self.sem.acquire().await?;
        
        // 执行 pkg 操作
        run_upgrade().await?;
        
        // 自动释放（permit drop 时）
        drop(permit);
        
        Ok(())
    }
}
```

**状态:** ❌ 未实现（可以实现排队机制）

---

## 方案 5: 等待队列 (Wait Queue) → 条件变量

**Linux 方式:**
```c
wait_queue_head_t wait;
init_waitqueue_head(&wait);

// 等待某个条件
wait_event_interruptible(wait, condition_is_true());

// 唤醒等待者
wake_up(&wait);
```

**对应到 Termux:**
```rust
use std::sync::CondVar;

pub struct PkgLock {
    locked: Mutex<bool>,
    cond: CondVar,
}

impl PkgLock {
    pub fn acquire(&self, session_id: usize) {
        let mut locked = self.locked.lock().unwrap();
        
        // 等待锁可用
        while *locked {
            locked = self.cond.wait(locked).unwrap();
        }
        
        *locked = true;
    }
    
    pub fn release(&self) {
        let mut locked = self.locked.lock().unwrap();
        *locked = false;
        self.cond.notify_one();  // ← 唤醒一个等待者
    }
}
```

**状态:** ❌ 未实现（可以实现等待队列）

---

## 📋 当前实现状态对比

| Linux 机制 | Termux 对应 | 状态 | 文件 |
|-----------|------------|------|------|
| 中断下半部 | 事件队列 | ✅ 已实现 | `lib.rs` |
| RCU | 原子操作 | ⚠️ 部分 | `coordinator.rs` |
| 无锁编程 | AtomicBool | ✅ 已实现 | `coordinator.rs` |
| 信号量 | - | ❌ 未实现 | - |
| 等待队列 | - | ❌ 未实现 | - |

---

## 🎯 可以改进的地方

### 1. 实现等待队列（类似 Linux wait_queue）

```rust
// coordinator.rs
use std::collections::VecDeque;
use std::sync::CondVar;

pub struct SessionCoordinator {
    pkg_lock: AtomicBool,
    pkg_lock_owner: AtomicUsize,
    // 新增：等待队列
    wait_queue: Mutex<VecDeque<usize>>,
    cond: CondVar,
}

impl SessionCoordinator {
    // 阻塞式获取锁（类似 down()）
    pub fn acquire_pkg_lock(&self, session_id: usize) {
        let mut queue = self.wait_queue.lock().unwrap();
        
        // 如果锁被占用，加入等待队列
        while self.pkg_lock.load(Ordering::SeqCst) {
            queue.push_back(session_id);
            queue = self.cond.wait(queue).unwrap();
        }
        
        // 获取锁
        self.pkg_lock.store(true, Ordering::SeqCst);
        self.pkg_lock_owner.store(session_id, Ordering::SeqCst);
    }
    
    // 释放锁并唤醒下一个（类似 up()）
    pub fn release_pkg_lock(&self, session_id: usize) {
        let owner = self.pkg_lock_owner.load(Ordering::SeqCst);
        if owner == session_id {
            self.pkg_lock.store(false, Ordering::SeqCst);
            
            // 唤醒等待者
            let mut queue = self.wait_queue.lock().unwrap();
            if let Some(next) = queue.pop_front() {
                // 直接转移锁所有权
                self.pkg_lock.store(true, Ordering::SeqCst);
                self.pkg_lock_owner.store(next, Ordering::SeqCst);
            }
            
            self.cond.notify_one();
        }
    }
}
```

### 2. 实现 RCU 风格的读侧优化

```rust
// engine.rs
use std::sync::atomic::{AtomicPtr, Ordering};

pub struct ScreenState {
    // 使用原子指针代替锁
    colors: AtomicPtr<ColorScheme>,
}

impl ScreenState {
    // 读侧 - 无锁
    pub fn get_colors(&self) -> &'static ColorScheme {
        unsafe {
            &*self.colors.load(Ordering::Acquire)
        }
    }
    
    // 写侧 - 复制 - 替换
    pub fn reset_colors(&self) {
        let new_colors = Box::new(ColorScheme::default());
        let old_colors = self.colors.swap(
            Box::into_raw(new_colors),
            Ordering::Release
        );
        
        // 延迟释放旧数据（类似 synchronize_rcu）
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(10));
            unsafe { drop(Box::from_raw(old_colors)); }
        });
    }
}
```

---

## ✅ 总结

**Termux-Rust 已经采用了 Linux 内核的核心思想:**

1. ✅ **中断下半部** → 事件队列（`flush_events_to_java`）
2. ✅ **无锁编程** → `AtomicBool` (pkg_lock)
3. ⚠️ **RCU** → 部分使用原子操作

**可以进一步改进:**

1. ❌ **等待队列** → 实现 pkg 操作排队机制
2. ❌ **完整 RCU** → 优化颜色读取性能
3. ❌ **信号量** → 限制并发 pkg 操作数量

**Linux 内核设计哲学的核心:**
- 能无锁就无锁
- 必须用锁时，尽量缩短持有时间
- 回调/中断处理中不获取锁
- 使用队列延迟处理

**这正是我们修复死锁的思路！** 🐧
