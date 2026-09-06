#!/usr/bin/env python3
"""Execute exact production finish/result/service methods in a Java boundary fixture.
Dependencies are explicit fixtures, not Android or full ExecutionCommand acceptance.
Actual APK compilation and Service runtime remain separate evidence layers.
"""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
source = (ROOT / 'termux-shared/src/main/java/com/termux/shared/termux/shell/command/runner/terminal/TermuxSession.java').read_text()
start = source.index('    public void finish()')
finish = source[start:source.index('    /**', start)]
start = source.index('    private static void processTermuxSessionResult(')
process = source[start:source.index('    public TerminalSession getTerminalSession()', start)]
service = (ROOT / 'app/src/main/java/com/termux/app/terminal/TermuxTerminalSessionServiceClient.java').read_text()
start = service.index('    public void onSessionFinished(')
service_method = service[start:service.index('    @Override', start)]
command = (ROOT / 'termux-shared/src/main/java/com/termux/shared/shell/command/ExecutionCommand.java').read_text()
start = command.index('    public synchronized boolean shouldNotProcessResults()')
latch = command[start:command.index('    public synchronized boolean isStateFailed()', start)]
fixture = '''
@interface NonNull {}
class Logger { static void logDebug(String t, String m) {} }
enum Errno { ERRNO_FAILED; int getCode() { return 1; } }
class TerminalSession {
 boolean running, alive=true; Integer actual=0; int compatibility=0; String error; Object facts=new Object();
 boolean isRunning() { return running; } int getExitStatus() { return compatibility; }
 Object getCompletionFacts() { return facts; } Integer getProcessExitStatus() { return actual; }
 String getCompletionError() { return error; }
}
class ShellUtils {
 static String getTerminalSessionTranscriptText(TerminalSession s, boolean a, boolean b) {
  if (!s.alive) throw new AssertionError("transcript after disposal"); return "tail+banner";
 }
}
class ExecutionCommand {
 enum ExecutionState { EXECUTED, SUCCESS }
 static class Data { Integer exitCode; StringBuilder stdout=new StringBuilder(); }
 final Data resultData=new Data(); boolean failed, processingResultsAlreadyCalled, plugin=true;
 String getCommandIdAndLabelLogString() { return "fixture"; }
 boolean isStateFailed() { return failed; }
 boolean setState(ExecutionState state) { return !failed; }
 boolean setStateFailed(int code, String text) { failed=true; return true; }
 boolean isPluginExecutionCommandWithPendingResult() { return plugin; }
''' + latch + '''
}
class TermuxSession {
 static final String LOG_TAG="fixture";
 TerminalSession mTerminalSession=new TerminalSession(); ExecutionCommand mExecutionCommand=new ExecutionCommand();
 boolean mSetStdoutOnExit=true; int results;
 interface Client { void onTermuxSessionExited(TermuxSession session); }
 Client mTermuxSessionClient = session -> {
  if (!session.mExecutionCommand.resultData.stdout.toString().equals("tail+banner")) throw new AssertionError("missing result before removal");
  session.results++; session.mTerminalSession.alive=false;
 };
 ExecutionCommand getExecutionCommand() { return mExecutionCommand; }
''' + finish + process + '''
}
class Service { TermuxSession session; TermuxSession getTermuxSessionForTerminalSession(TerminalSession t) { return session; } }
class ServiceClient {
 final Service mService=new Service();
''' + service_method + '''
}
public class CompletionResultFixture {
 static void check(boolean b) { if (!b) throw new AssertionError(); }
 public static void main(String[] args) {
  for (int code : new int[]{0,37,-9}) {
   TermuxSession s=new TermuxSession(); s.mTerminalSession.actual=code; s.mTerminalSession.compatibility=code;
   s.finish(); check(s.results==1 && !s.mExecutionCommand.failed && s.mExecutionCommand.resultData.exitCode==code);
   // Result processing latch belongs only to processTermuxSessionResult.
   s.mTerminalSession.alive=true; s.finish(); check(s.results==1);
  }
  TermuxSession io=new TermuxSession(); io.mTerminalSession.error="IO error"; io.finish();
  check(io.results==1 && io.mExecutionCommand.failed && io.mExecutionCommand.resultData.exitCode==0);
  TermuxSession lost=new TermuxSession(); lost.mTerminalSession.actual=null; lost.mTerminalSession.compatibility=1;
  lost.mTerminalSession.error="status lost"; lost.finish();
  check(lost.results==1 && lost.mExecutionCommand.failed && lost.mExecutionCommand.resultData.exitCode==null);
  TermuxSession running=new TermuxSession(); running.mTerminalSession.running=true; running.finish(); check(running.results==0);
  TermuxSession prior=new TermuxSession(); prior.mExecutionCommand.failed=true; prior.finish(); check(prior.results==0);
  ServiceClient service=new ServiceClient(); service.mService.session=new TermuxSession();
  service.onSessionFinished(service.mService.session.mTerminalSession); check(service.mService.session.results==1);
  service.mService.session=new TermuxSession(); service.mService.session.mExecutionCommand.plugin=false;
  service.onSessionFinished(service.mService.session.mTerminalSession); check(service.mService.session.results==0);
  System.out.println("PASS exact production finish/result/service methods: real codes, lost=null, IO failure, capture-before-dispose, latch, plugin-only service delivery");
  System.out.println("BOUNDARY: Java method extraction + explicit dependency fixtures; not Android Service runtime or full ExecutionCommand");
 }
}
'''
with tempfile.TemporaryDirectory(prefix='completion-result-') as directory:
    path = Path(directory)
    (path / 'CompletionResultFixture.java').write_text(fixture)
    subprocess.run(['javac', '-d', directory, str(path / 'CompletionResultFixture.java')], check=True)
    subprocess.run(['java', '-cp', directory, 'CompletionResultFixture'], check=True)
