using Xunit;

// Linux tests share global AppPaths state — same disable here for consistency
// và an toàn nếu sau này thêm test chạm static state.
[assembly: CollectionBehavior(DisableTestParallelization = true)]
