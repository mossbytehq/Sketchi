using Microsoft.UI.Xaml;

namespace Sketchi.Bootstrapper;

public sealed partial class App : Application
{
    private readonly SketchiBootstrapperApplication bootstrapper;
    private MainWindow? window;

    public App(SketchiBootstrapperApplication bootstrapper)
    {
        this.bootstrapper = bootstrapper;
        InitializeComponent();

        window = new MainWindow(bootstrapper);
        window.Activate();
        bootstrapper.AttachWindow(window);
    }
}
