#include "catch.hpp"
#include "test_helpers.hpp"
#include "duckdb/parallel/task_executor.hpp"

#include <chrono>
#include <future>
#include <thread>

using namespace duckdb;

struct WeirdTask : BaseExecutorTask {
	using BaseExecutorTask::BaseExecutorTask;

	void ExecuteTask() override {
	}

	TaskExecutionResult Execute(TaskExecutionMode mode) override {
		if (mode == TaskExecutionMode::PROCESS_PARTIAL) {
			std::this_thread::sleep_for(std::chrono::milliseconds(300));
			return TaskExecutionResult::TASK_NOT_FINISHED;
		}
		executor.FinishTask();
		return TaskExecutionResult::TASK_FINISHED;
	}
};

struct BlockingTask : BaseExecutorTask {
	BlockingTask(TaskExecutor &executor, std::promise<void> &started_p, std::shared_future<void> release_p)
	    : BaseExecutorTask(executor), started(started_p), release(std::move(release_p)) {
	}

	void ExecuteTask() override {
		started.set_value();
		release.wait();
	}

	std::promise<void> &started;
	std::shared_future<void> release;
};

TEST_CASE("TaskExecutor can execute partial tasks without busy spinning forever") {
	DuckDB db;
	Connection con {db};
	REQUIRE_NO_FAIL(con.Query("SET threads=5"));
	REQUIRE_NO_FAIL(con.Query("SET scheduler_process_partial=true"));
	TaskExecutor executor {*con.context};

	// One task per background worker (threads=5, external=1 -> 4 workers).
	for (auto i = 0; i < 4; i++) {
		executor.ScheduleTask(make_uniq<WeirdTask>(executor));
	}

	// Let each worker grab a task and enter its PROCESS_PARTIAL sleep.
	std::this_thread::sleep_for(std::chrono::milliseconds(100));

	// WorkOnTasks finds the producer queue empty (all tasks in worker hands),
	// exits its first loop, and enters `while (completed_tasks != total_tasks) {}`.
	auto finished = std::async(std::launch::async, [&] { executor.WorkOnTasks(); });

	// Kill background workers. Their in-flight tasks get re-enqueued as
	// TASK_NOT_FINISHED and stranded — WorkOnTasks is already busy-spinning
	// and never re-checks the queue.
	std::this_thread::sleep_for(std::chrono::milliseconds(50));
	REQUIRE_NO_FAIL(con.Query("SET threads=1"));

	REQUIRE(finished.wait_for(std::chrono::milliseconds(100)) == std::future_status::ready);
}

TEST_CASE("TaskExecutor wakes all concurrent WorkOnTasks waiters") {
	DuckDB db;
	Connection con {db};
	REQUIRE_NO_FAIL(con.Query("SET threads=1"));
	TaskExecutor executor {*con.context};

	std::promise<void> task_started;
	auto task_started_future = task_started.get_future();
	std::promise<void> release_task;
	executor.ScheduleTask(make_uniq<BlockingTask>(executor, task_started, release_task.get_future().share()));

	std::promise<void> first_started;
	std::promise<void> second_started;
	std::promise<void> third_started;
	auto first_started_future = first_started.get_future();
	auto second_started_future = second_started.get_future();
	auto third_started_future = third_started.get_future();
	auto first = std::async(std::launch::async, [&] {
		first_started.set_value();
		executor.WorkOnTasks();
	});
	auto second = std::async(std::launch::async, [&] {
		second_started.set_value();
		executor.WorkOnTasks();
	});
	auto third = std::async(std::launch::async, [&] {
		third_started.set_value();
		executor.WorkOnTasks();
	});

	first_started_future.wait();
	second_started_future.wait();
	third_started_future.wait();
	task_started_future.wait();
	// Allow the remaining callers to reach the producer condition-variable wait.
	std::this_thread::sleep_for(std::chrono::milliseconds(100));
	release_task.set_value();

	REQUIRE(first.wait_for(std::chrono::seconds(1)) == std::future_status::ready);
	REQUIRE(second.wait_for(std::chrono::seconds(1)) == std::future_status::ready);
	REQUIRE(third.wait_for(std::chrono::seconds(1)) == std::future_status::ready);
}
