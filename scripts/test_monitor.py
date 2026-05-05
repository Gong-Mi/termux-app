import subprocess
import re
import sys

def run_tests():
    cmd = ["cargo", "test", "--all-features", "--release", "--", "--test-threads=1"]
    print(f"Executing: {' '.join(cmd)}")
    print("-" * 60)

    import os
    # 获取脚本所在目录的上一级（项目根目录）
    base_dir = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    rust_dir = os.path.join(base_dir, "terminal-emulator/src/main/rust")
    
    process = subprocess.Popen(
        cmd,
        cwd=rust_dir,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,  # 合并输出流以保持顺序
        text=True,
        bufsize=1
    )

    passed_count = 0
    failed_tests = []
    warnings = []
    
    # 实时处理合并后的输出流
    for line in process.stdout:
        line = line.strip()
        if not line: continue
        
        if "warning:" in line:
            warnings.append(line)
            print(f"\033[93m[WARN]\033[0m {line}")
        elif "error:" in line:
            print(f"\033[91m[ERR ]\033[0m {line}")
        elif line.startswith("test ") and " ... ok" in line:
            passed_count += 1
            if passed_count % 50 == 0:
                print(f"\033[92m[PASS]\033[0m 已通过 {passed_count} 个测试...")
        elif " ... FAILED" in line:
            parts = line.split()
            if len(parts) > 1:
                test_name = parts[1]
                failed_tests.append(test_name)
                print(f"\033[91m[FAIL]\033[0m {test_name}")
        elif "test result:" in line:
            print(f"\n\033[1m{line}\033[0m")
        elif "Running unittests" in line or "Running tests" in line:
            print(f"\033[36m>>> {line}\033[0m")

    process.wait()

    print("\n" + "="*60)
    print(" 🔍 测试运行总结")
    print("="*60)
    print(f"✅ 通过总数: {passed_count}")
    print(f"⚠️ 警告总数: {len(warnings)}")
    
    if failed_tests:
        print(f"❌ 失败测试 ({len(failed_tests)}):")
        for ft in failed_tests:
            print(f"   - {ft}")
    else:
        print("🎉 没有失败的测试项目！")

    if warnings:
        print(f"\n💡 前 5 条警告详情:")
        for w in warnings[:5]:
            print(f"   {w}")

if __name__ == "__main__":
    run_tests()
