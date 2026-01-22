# 运行所有测试

## 快速测试命令

```bash
# 1. 运行所有单元测试和集成测试
cargo test --lib --bins --tests

# 2. 运行属性测试（Property-Based Tests）
cargo test --test property_tests
cargo test --test wal_property_tests  
cargo test --test config_property_tests
cargo test --test engine_property_tests

# 3. 运行性能基准测试
cargo bench

# 4. 运行所有测试（一次性）
cargo test --all
```

## 测试分类

### 单元测试 (17个)
```bash
cargo test --lib
```
- 配置验证测试
- 订单类型测试
- 订单簿基本操作测试
- 计时器测试

### 集成测试 (38个)
```bash
cargo test --test basic_tests
cargo test --test comprehensive_tests
cargo test --test integration_tests
```
- 基础功能测试 (10个)
- 全面功能测试 (20个)
- 系统集成测试 (8个)

### 属性测试 (49个)
```bash
# 核心撮合属性 (15个)
cargo test --test property_tests

# WAL 持久化属性 (8个)
cargo test --test wal_property_tests

# 配置管理属性 (14个)
cargo test --test config_property_tests

# 引擎统计属性 (12个)
cargo test --test engine_property_tests
```

### 性能基准测试
```bash
cargo bench --bench matching_benchmark
cargo bench --bench comprehensive_benchmarks
```

## 预期结果

### 总测试数量
- **单元测试**: 17个
- **集成测试**: 38个  
- **属性测试**: 49个
- **文档测试**: 2个
- **总计**: 106+ 个测试

### 所有测试应该通过
```
test result: ok. 106 passed; 0 failed; 0 ignored
```

## 持续集成

在 CI/CD 管道中运行：

```bash
#!/bin/bash
set -e

echo "Running all tests..."

# 单元测试和集成测试
cargo test --lib --bins --tests

# 属性测试
cargo test --test property_tests
cargo test --test wal_property_tests
cargo test --test config_property_tests
cargo test --test engine_property_tests

# 代码质量检查
cargo clippy -- -D warnings
cargo fmt --check

echo "All tests passed!"
```

## 故障排查

如果测试失败：

1. **查看详细输出**:
   ```bash
   cargo test -- --nocapture
   ```

2. **运行特定测试**:
   ```bash
   cargo test test_name -- --nocapture
   ```

3. **查看 PropTest 失败案例**:
   ```bash
   cat tests/*.proptest-regressions
   ```

4. **重新运行失败的测试**:
   ```bash
   cargo test --test property_tests -- --ignored
   ```
