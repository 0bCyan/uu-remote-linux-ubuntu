import importlib.util,pathlib,unittest
spec=importlib.util.spec_from_file_location('audit',pathlib.Path(__file__).with_name('endurance-audit.py'));module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
class TestAudit(unittest.TestCase):
 def make(self,progress=True):
  audit=module.Audit(0,3600)
  for stamp in range(0,3601,30):audit.accept(stamp,f'Video receiver report: ssrc=99 last_sequence={stamp if progress else 1}')
  audit.accept(0,'ICE path: session=1 route=relay local=udp/relay remote=udp/host')
  audit.accept(900,'Room renewal deferred while remote connection is active')
  return audit
 def test_receiver_heartbeats_without_new_video_fail(self):self.assertFalse(self.make(False).result()['passed'])
 def test_local_relay_progress_and_deferrals_pass(self):self.assertTrue(self.make().result()['passed'])
 def test_peer_replacement_and_remote_only_relay_do_not_pass(self):
  audit=self.make();audit.accept(1000,'ICE path: session=2 route=relay local=udp/relay remote=udp/host');self.assertFalse(audit.result()['passed'])
  audit=self.make();audit.local_kinds={'srflx'};self.assertFalse(audit.result()['passed'])
 def test_turn_refresh_errors_alone_do_not_mean_video_failure(self):
  audit=self.make();audit.accept(120,'fail to refresh permissions: error 400');self.assertTrue(audit.result()['passed']);self.assertEqual(audit.result()['turn_permission_refresh_errors'],1)
if __name__=='__main__':unittest.main()
