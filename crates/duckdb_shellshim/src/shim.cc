#include <cstddef>
#include <cstring>
#include <string>
#include "duckdb.hpp"
#include "duckdb/common/box_renderer.hpp"
#include "duckdb/common/vector_operations/vector_operations.hpp"
#include "duckdb/parser/keyword_helper.hpp"

extern "C" {

const char *duckdb_shellshim_target_version() {
	return "1.4.3";
}

const char *duckdb_shellshim_library_version() {
	return duckdb::DuckDB::LibraryVersion();
}

const char *duckdb_shellshim_release_codename() {
	return duckdb::DuckDB::ReleaseCodename();
}

const char *duckdb_shellshim_source_id() {
	return duckdb::DuckDB::SourceID();
}

int duckdb_shellshim_keyword_check(const char *str, size_t len) {
	if (!str) {
		return 0;
	}
	try {
		return duckdb::KeywordHelper::IsKeyword(std::string(str, len)) ? 1 : 0;
	} catch (...) {
		return 0;
	}
}

struct duckdb_shellshim_duckbox_config {
	uint64_t max_rows;
	uint64_t max_width;
	uint64_t max_analyze_rows;
	const char *null_value;
	bool columns;
	char decimal_separator;
	char thousand_separator;
	int large_number_rendering; // 0=none,1=footer,2=all,3=default
	bool stdout_is_console;
	bool output_is_file;
	bool highlight_results;
	const char *ansi_column_name;
	const char *ansi_column_type;
	const char *ansi_null_value;
	const char *ansi_reset;
};

class ShellShimAnsiBoxRenderer : public duckdb::BaseResultRenderer {
public:
	explicit ShellShimAnsiBoxRenderer(const duckdb_shellshim_duckbox_config &cfg) : cfg(cfg) {
	}

	void RenderLayout(const std::string &text) override {
		// Match shell highlight behavior: when result highlighting is enabled, render layout
		// (box drawing chars, padding, separators) in the same style as column types.
		Append(text, cfg.ansi_column_type);
	}
	void RenderColumnName(const std::string &text) override {
		Append(text, nullptr);
	}
	void RenderType(const std::string &text) override {
		Append(text, nullptr);
	}
	void RenderValue(const std::string &text, const duckdb::LogicalType & /*type*/) override {
		Append(text, nullptr);
	}
	void RenderNull(const std::string &text, const duckdb::LogicalType & /*type*/) override {
		Append(text, cfg.ansi_null_value);
	}
	void RenderFooter(const std::string &text) override {
		Append(text, nullptr);
	}

	const std::string &str() const {
		return result;
	}

private:
	void Append(const std::string &text, const char *ansi_code) {
		if (cfg.highlight_results && ansi_code && *ansi_code && cfg.ansi_reset) {
			result += ansi_code;
			result += text;
			result += cfg.ansi_reset;
		} else {
			result += text;
		}
	}

	const duckdb_shellshim_duckbox_config &cfg;
	std::string result;
};

// Returns 0 on success, non-zero on error.
int duckdb_shellshim_render_duckbox(duckdb_connection connection, const char *query,
                                   const duckdb_shellshim_duckbox_config *cfg, char **out_rendered,
                                   char **out_error) {
	if (!connection || !query || !cfg || !out_rendered) {
		if (out_error) {
			*out_error = strdup("duckdb_shellshim_render_duckbox: invalid arguments");
		}
		return 1;
	}

	try {
		auto &con = *reinterpret_cast<duckdb::Connection *>(connection);
		auto statements = con.ExtractStatements(query);

		duckdb::BoxRendererConfig config;
		config.max_rows = static_cast<duckdb::idx_t>(cfg->max_rows);
		config.max_width = static_cast<duckdb::idx_t>(cfg->max_width);

		if (config.max_width == 0) {
			if (cfg->output_is_file) {
				config.max_rows = (size_t)-1;
				config.max_width = (size_t)-1;
			}
			if (!cfg->stdout_is_console) {
				config.max_width = (size_t)-1;
			}
		}

		duckdb::LargeNumberRendering large_rendering;
		if (cfg->large_number_rendering == 3) {
			large_rendering =
			    cfg->stdout_is_console ? duckdb::LargeNumberRendering::FOOTER : duckdb::LargeNumberRendering::NONE;
		} else {
			large_rendering = static_cast<duckdb::LargeNumberRendering>(cfg->large_number_rendering);
		}

		if (cfg->null_value) {
			config.null_value = cfg->null_value;
		}
		if (cfg->columns) {
			config.render_mode = duckdb::RenderMode::COLUMNS;
		}
		config.decimal_separator = cfg->decimal_separator;
		config.thousand_separator = cfg->thousand_separator;
		config.large_number_rendering = large_rendering;

		std::string combined;
		for (auto &stmt : statements) {
			auto prepared = con.Prepare(std::move(stmt));
			if (!prepared) {
				if (out_error) {
					*out_error = strdup("duckdb_shellshim_render_duckbox: prepare returned null");
				}
				return 1;
			}
			if (prepared->HasError()) {
				if (out_error) {
					*out_error = strdup(prepared->GetError().c_str());
				}
				return 1;
			}
			duckdb::vector<duckdb::Value> values;
			auto exec_fn = static_cast<duckdb::unique_ptr<duckdb::QueryResult> (duckdb::PreparedStatement::*)(
			    duckdb::vector<duckdb::Value> &, bool)>(&duckdb::PreparedStatement::Execute);
			auto result = (prepared.get()->*exec_fn)(values, false);
			if (!result) {
				if (out_error) {
					*out_error = strdup("duckdb_shellshim_render_duckbox: execute returned null result");
				}
				return 1;
			}
				if (result->HasError()) {
					if (out_error) {
						auto err = result->ToString();
						*out_error = strdup(err.c_str());
					}
					return 1;
				}
				// Match DuckDB CLI behavior: statements that do not return a query result
				// (e.g., CREATE/INSERT) should not render a "Count" table.
				if (result->properties.return_type != duckdb::StatementReturnType::QUERY_RESULT) {
					continue;
				}
				auto &materialized = result->Cast<duckdb::MaterializedQueryResult>();
				duckdb::BoxRenderer renderer(config);
				ShellShimAnsiBoxRenderer out(*cfg);
				renderer.Render(*con.context, result->names, materialized.Collection(), out);
				combined += out.str();
		}

		*out_rendered = strdup(combined.c_str());
		return 0;
	} catch (std::exception &ex) {
		if (out_error) {
			*out_error = strdup(ex.what());
		}
		return 1;
	} catch (...) {
		if (out_error) {
			*out_error = strdup("duckdb_shellshim_render_duckbox: unknown error");
		}
		return 1;
	}
}

// Returns 0 on success, non-zero on error.
int duckdb_shellshim_cast_chunk_to_varchar(duckdb_connection connection, duckdb_data_chunk chunk,
                                          duckdb_data_chunk *out_chunk, char **out_error) {
	if (!connection || !chunk || !out_chunk) {
		if (out_error) {
			*out_error = strdup("duckdb_shellshim_cast_chunk_to_varchar: invalid arguments");
		}
		return 1;
	}
	try {
		auto &con = *reinterpret_cast<duckdb::Connection *>(connection);
		auto &context = *con.context;
		auto &input = *reinterpret_cast<duckdb::DataChunk *>(chunk);

		duckdb::vector<duckdb::LogicalType> varchar_types;
		varchar_types.reserve(input.ColumnCount());
		for (duckdb::idx_t c = 0; c < input.ColumnCount(); c++) {
			varchar_types.emplace_back(duckdb::LogicalType(duckdb::LogicalTypeId::VARCHAR));
		}

		auto out = duckdb::make_uniq<duckdb::DataChunk>();
		out->Initialize(duckdb::Allocator::DefaultAllocator(), varchar_types);

		auto count = input.size();
		for (duckdb::idx_t c = 0; c < input.ColumnCount(); c++) {
			duckdb::VectorOperations::Cast(context, input.data[c], out->data[c], count);
		}
		out->SetCardinality(count);

		*out_chunk = reinterpret_cast<duckdb_data_chunk>(out.release());
		return 0;
	} catch (std::exception &ex) {
		if (out_error) {
			*out_error = strdup(ex.what());
		}
		return 1;
	} catch (...) {
		if (out_error) {
			*out_error = strdup("duckdb_shellshim_cast_chunk_to_varchar: unknown error");
		}
		return 1;
	}
}

size_t duckdb_shellshim_render_width_fallback(const char *str, size_t str_len) {
	(void)str;
	return str_len;
}

} // extern "C"
