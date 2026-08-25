using WixToolset.Mba.Core;

namespace Sketchi.Bootstrapper;

internal static class Program
{
    [STAThread]
    private static int Main()
    {
        ManagedBootstrapperApplication.Run(new SketchiBootstrapperApplication());
        return 0;
    }
}
