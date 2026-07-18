//===----------------------------------------------------------------------===//
//                         DuckDB
//
// Shared test utilities for external file cache tests.
//
//===----------------------------------------------------------------------===//

#pragma once

#include "duckdb/common/file_system.hpp"
#include "duckdb/common/local_file_system.hpp"
#include "duckdb/common/mutex.hpp"
#include "duckdb/common/string.hpp"
#include "duckdb/common/thread_annotation.hpp"
#include "test_helpers.hpp"

namespace duckdb {

class CachingTestFileGuard {
public:
	CachingTestFileGuard(const string &filename, const string &content);
	~CachingTestFileGuard();

	const string &GetPath() const;

private:
	string file_path;
};

class SimpleTrackingFileSystem : public LocalFileSystem {
public:
	string GetName() const override;
	bool CanHandleFile(const string &path) override;
	bool CanSeek() override;
	string GetVersionTag(FileHandle &handle) override;
};

//! A file system that returns no ETag and timestamp_t(0) for Last-Modified, simulating servers that do not
//! provide cache-validation headers.
class NoValidationMetadataFileSystem : public LocalFileSystem {
public:
	string GetName() const override;
	bool CanHandleFile(const string &path) override;
	bool CanSeek() override;
	string GetVersionTag(FileHandle &handle) override;
	timestamp_t GetLastModifiedTime(FileHandle &handle) override;
};

//! A local-backed file system that simulates remote paths and tracks positional reads.
class RemoteTrackingFileSystem : public SimpleTrackingFileSystem {
public:
	string GetName() const override;
	bool CanHandleFile(const string &path) override;
	unique_ptr<FileHandle> OpenFile(const string &path, FileOpenFlags flags,
	                                optional_ptr<FileOpener> opener = nullptr) override;
	void Read(FileHandle &handle, void *buffer, int64_t nr_bytes, idx_t location) override;

	void ResetReadStats();
	idx_t GetReadCount() const;
	idx_t GetReadBytes() const;

private:
	mutable annotated_mutex read_stats_mutex;
	idx_t read_count DUCKDB_GUARDED_BY(read_stats_mutex) = 0;
	idx_t read_bytes DUCKDB_GUARDED_BY(read_stats_mutex) = 0;
};

//! In-memory DuckDB with the external file cache forced to also cache local files (off by default), so the external
//! file cache tests can exercise the cache machinery on local temp files.
DuckDB MakeCacheLocalFilesDB();

} // namespace duckdb
