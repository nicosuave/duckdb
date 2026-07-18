#include "caching_test_utils.hpp"

#include "duckdb/common/string_util.hpp"

namespace duckdb {

CachingTestFileGuard::CachingTestFileGuard(const string &filename, const string &content)
    : file_path(TestCreatePath(filename)) {
	auto local_fs = FileSystem::CreateLocal();
	auto handle = local_fs->OpenFile(file_path, FileFlags::FILE_FLAGS_WRITE | FileFlags::FILE_FLAGS_FILE_CREATE);
	handle->Write(QueryContext(), const_cast<char *>(content.data()), content.size(), 0);
	handle->Sync();
}

CachingTestFileGuard::~CachingTestFileGuard() {
	auto local_fs = FileSystem::CreateLocal();
	local_fs->TryRemoveFile(file_path);
}

const string &CachingTestFileGuard::GetPath() const {
	return file_path;
}

DuckDB MakeCacheLocalFilesDB() {
	DBConfig config;
	config.SetOptionByName("cache_local_files", true);
	return DuckDB(":memory:", &config);
}

string SimpleTrackingFileSystem::GetName() const {
	return "TrackingFileSystem";
}

bool SimpleTrackingFileSystem::CanHandleFile(const string &path) {
	return StringUtil::StartsWith(path, TestDirectoryPath());
}

bool SimpleTrackingFileSystem::CanSeek() {
	return true;
}

string SimpleTrackingFileSystem::GetVersionTag(FileHandle &handle) {
	return StringUtil::Format("%lld:%lld", GetFileSize(handle), GetLastModifiedTime(handle).value);
}

string NoValidationMetadataFileSystem::GetName() const {
	return "NoValidationMetadataFileSystem";
}

bool NoValidationMetadataFileSystem::CanHandleFile(const string &path) {
	return StringUtil::StartsWith(path, TestDirectoryPath());
}

bool NoValidationMetadataFileSystem::CanSeek() {
	return true;
}

string NoValidationMetadataFileSystem::GetVersionTag(FileHandle &handle) {
	return "";
}

timestamp_t NoValidationMetadataFileSystem::GetLastModifiedTime(FileHandle &handle) {
	return timestamp_t(0);
}

string RemoteTrackingFileSystem::GetName() const {
	return "RemoteTrackingFileSystem";
}

bool RemoteTrackingFileSystem::CanHandleFile(const string &path) {
	return StringUtil::StartsWith(path, "s3://");
}

unique_ptr<FileHandle> RemoteTrackingFileSystem::OpenFile(const string &path, FileOpenFlags flags,
                                                          optional_ptr<FileOpener> opener) {
	D_ASSERT(CanHandleFile(path));
	return LocalFileSystem::OpenFile(path.substr(5), flags, opener);
}

void RemoteTrackingFileSystem::Read(FileHandle &handle, void *buffer, int64_t nr_bytes, idx_t location) {
	{
		const annotated_lock_guard<annotated_mutex> guard(read_stats_mutex);
		read_count++;
		read_bytes += UnsafeNumericCast<idx_t>(nr_bytes);
	}
	LocalFileSystem::Read(handle, buffer, nr_bytes, location);
}

void RemoteTrackingFileSystem::ResetReadStats() {
	const annotated_lock_guard<annotated_mutex> guard(read_stats_mutex);
	read_count = 0;
	read_bytes = 0;
}

idx_t RemoteTrackingFileSystem::GetReadCount() const {
	const annotated_lock_guard<annotated_mutex> guard(read_stats_mutex);
	return read_count;
}

idx_t RemoteTrackingFileSystem::GetReadBytes() const {
	const annotated_lock_guard<annotated_mutex> guard(read_stats_mutex);
	return read_bytes;
}

} // namespace duckdb
