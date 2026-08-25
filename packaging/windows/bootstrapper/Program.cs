using System.Text;
using WixToolset.BootstrapperApplicationApi;

namespace Sketchi.Bootstrapper;

internal static class Program
{
    [STAThread]
    private static int Main()
    {
        try
        {
            ManagedBootstrapperApplication.Run(new SketchiBootstrapperApplication());
            return 0;
        }
        catch (Exception error)
        {
            WriteStartupFailure(error);
            return 1;
        }
    }

    private static void WriteStartupFailure(Exception error)
    {
        var path = Environment.GetEnvironmentVariable("SKETCHI_BOOTSTRAPPER_DIAGNOSTIC_LOG");
        if (string.IsNullOrWhiteSpace(path))
        {
            path = Path.Combine(Path.GetTempPath(), "Sketchi-Bootstrapper-startup.log");
        }

        try
        {
            File.AppendAllText(
                path,
                $"[{DateTimeOffset.UtcNow:O}] Bootstrapper startup failed{Environment.NewLine}"
                    + $"{error}{Environment.NewLine}",
                Encoding.UTF8);
        }
        catch
        {
            // Startup diagnostics must never mask the original bootstrapper failure.
        }
    }
}
