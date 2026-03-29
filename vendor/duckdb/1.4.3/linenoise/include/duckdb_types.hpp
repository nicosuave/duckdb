#pragma once

#include <cstdint>
#include <string>
#include <vector>

namespace duckdb {
using idx_t = uint64_t;
using string = std::string;

template <class T>
using vector = std::vector<T>;
} // namespace duckdb

