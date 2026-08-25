using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI;
using Microsoft.UI.Windowing;
using WinRT.Interop;

namespace Sketchi.Bootstrapper;

public sealed partial class MainWindow : Window
{
    private readonly SketchiBootstrapperApplication bootstrapper;
    private bool allowBootstrapperClose;

    public MainWindow(SketchiBootstrapperApplication bootstrapper)
    {
        this.bootstrapper = bootstrapper;
        InitializeComponent();

        ExtendsContentIntoTitleBar = true;
        SetTitleBar(AppTitleBar);
        var handle = WindowNative.GetWindowHandle(this);
        var windowId = Win32Interop.GetWindowIdFromWindow(handle);
        AppWindow.GetFromWindowId(windowId).Closing += MainWindow_Closing;

        HeadingText.Text = bootstrapper.IsUninstall ? "Uninstall Sketchi" : "Install Sketchi";
        InstallButton.Content = bootstrapper.IsUninstall ? "Uninstall" : "Install";
    }

    internal void SetStatus(string message)
    {
        DispatcherQueue.TryEnqueue(() => StatusText.Text = message);
    }

    internal void SetApplying(bool applying)
    {
        DispatcherQueue.TryEnqueue(() =>
        {
            InstallButton.IsEnabled = !applying;
            CancelButton.IsEnabled = !applying;
            ProgressBar.Visibility = applying ? Visibility.Visible : Visibility.Collapsed;
            if (applying)
            {
                StatusText.Text = bootstrapper.IsUninstall
                    ? "Removing Sketchi…"
                    : "Installing Sketchi…";
            }
        });
    }

    internal void CloseFromBootstrapper()
    {
        allowBootstrapperClose = true;
        DispatcherQueue.TryEnqueue(() =>
        {
            Close();
            Application.Current.Exit();
        });
    }

    private void InstallButton_Click(object sender, RoutedEventArgs e)
    {
        bootstrapper.BeginApply();
    }

    private void CancelButton_Click(object sender, RoutedEventArgs e)
    {
        bootstrapper.Cancel();
    }

    private void MainWindow_Closing(AppWindow sender, AppWindowClosingEventArgs args)
    {
        if (bootstrapper.IsApplying && !allowBootstrapperClose)
        {
            args.Cancel = true;
            return;
        }

        bootstrapper.Cancel();
    }
}
