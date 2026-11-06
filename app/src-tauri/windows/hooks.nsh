; The tunnel service, registered and removed with the app.
;
; wandererd runs as LocalSystem, started at boot, and restarts if it fails:
; while it is down nothing removes a guard it left, so the machine stays
; offline rather than leaking. The installer runs elevated (perMachine).

!macro NSIS_HOOK_PREINSTALL
  ; An update replaces wandererd.exe and xray.exe, which the service holds.
  ; Stopping it takes the tunnel and the guard down first.
  nsExec::ExecToLog 'sc.exe stop wandererd'
  Sleep 2000
!macroend

!macro NSIS_HOOK_POSTINSTALL
  nsExec::ExecToLog 'sc.exe delete wandererd'
  Sleep 500
  nsExec::ExecToLog 'sc.exe create wandererd binPath= "\"$INSTDIR\wandererd.exe\"" start= auto DisplayName= "Wanderer tunnel service"'
  nsExec::ExecToLog 'sc.exe description wandererd "Runs the Wanderer tunnel and keeps traffic from leaving outside it."'
  nsExec::ExecToLog 'sc.exe failure wandererd reset= 60 actions= restart/2000/restart/2000/restart/10000'
  nsExec::ExecToLog 'sc.exe start wandererd'
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  nsExec::ExecToLog 'sc.exe stop wandererd'
  Sleep 2000
  nsExec::ExecToLog 'sc.exe delete wandererd'
!macroend
