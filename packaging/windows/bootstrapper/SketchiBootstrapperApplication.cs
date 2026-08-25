using System.Threading;
using Microsoft.UI.Xaml;
using WinRT.Interop;
using WixToolset.BootstrapperApplicationApi;

namespace Sketchi.Bootstrapper;

public sealed class SketchiBootstrapperApplication : BootstrapperApplication
{
    private IBootstrapperCommand command = null!;
    private readonly ManualResetEventSlim completion = new();
    private MainWindow? window;
    private nint windowHandle;
    private bool applying;
    private bool nonInteractiveMode;

    internal IBootstrapperCommand Command => command;
    internal IEngine Engine => engine;

    public bool IsUninstall => Command.Action == LaunchAction.Uninstall;

    public SketchiBootstrapperApplication()
    {
        DetectComplete += OnDetectComplete;
        PlanComplete += OnPlanComplete;
        ApplyComplete += OnApplyComplete;
    }

    protected override void OnCreate(CreateEventArgs args)
    {
        base.OnCreate(args);
        command = args.Command;
    }

    protected override void Run()
    {
        // Burn uses None for quiet execution and Passive for non-interactive
        // progress. Treat every mode other than Full as non-interactive so
        // command-line automation never accidentally starts a second UI loop.
        nonInteractiveMode = Command.Display != Display.Full;
        if (nonInteractiveMode)
        {
            Engine.Detect();
            completion.Wait();
            return;
        }

        var uiThread = new Thread(() =>
        {
            WinRT.ComWrappersSupport.InitializeComWrappers();
            Application.Start(_ =>
            {
                new App(this);
            });
        });
        uiThread.SetApartmentState(ApartmentState.STA);
        uiThread.IsBackground = false;
        uiThread.Start();
        uiThread.Join();
    }

    internal void AttachWindow(MainWindow window)
    {
        this.window = window;
        windowHandle = WindowNative.GetWindowHandle(window);
        Engine.CloseSplashScreen();

        if (Command.Action == LaunchAction.Help)
        {
            Quit(0);
            return;
        }

        window.SetStatus("Checking the current installation…");
        Engine.Detect();
    }

    internal bool IsApplying => applying;

    internal void BeginApply()
    {
        if (applying)
        {
            return;
        }

        applying = true;
        window?.SetApplying(true);
        Engine.Plan(IsUninstall ? LaunchAction.Uninstall : LaunchAction.Install);
    }

    internal void Cancel()
    {
        if (!applying)
        {
            Quit(0);
        }
    }

    private void OnDetectComplete(object? sender, DetectCompleteEventArgs args)
    {
        if (args.Status < 0)
        {
            ShowFailure($"Could not inspect the existing installation (0x{args.Status:X8}).");
            return;
        }

        if (nonInteractiveMode)
        {
            Engine.Plan(IsUninstall ? LaunchAction.Uninstall : LaunchAction.Install);
            return;
        }

        window?.SetStatus(IsUninstall
            ? "Ready to remove Sketchi."
            : "Ready to install Sketchi.");
    }

    private void OnPlanComplete(object? sender, PlanCompleteEventArgs args)
    {
        if (args.Status < 0)
        {
            ShowFailure($"Could not prepare the installation (0x{args.Status:X8}).");
            return;
        }

        Engine.Apply(nonInteractiveMode ? nint.Zero : windowHandle);
    }

    private void OnApplyComplete(object? sender, ApplyCompleteEventArgs args)
    {
        if (args.Status < 0)
        {
            ShowFailure($"Setup failed (0x{args.Status:X8}).");
            return;
        }

        if (!nonInteractiveMode)
        {
            window?.SetStatus(IsUninstall ? "Sketchi was removed." : "Sketchi was installed.");
            window?.CloseFromBootstrapper();
        }
        Quit(args.Status < 0 ? args.Status : 0);
    }

    private void ShowFailure(string message)
    {
        applying = false;
        if (nonInteractiveMode)
        {
            Quit(1);
            return;
        }
        window?.SetApplying(false);
        window?.SetStatus(message);
    }

    private void Quit(int exitCode)
    {
        completion.Set();
        Engine.Quit(exitCode);
    }
}
